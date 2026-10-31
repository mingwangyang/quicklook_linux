//! Thunar 适配：LD_PRELOAD 注入 Thunar，把「空格预览」接管为调用 Quick Look。
//!
//! Thunar 4.18 用 `GtkApplication` 启动，`g_application_run` 是跨 DSO 调用、会走 PLT，
//! 因此可以在 LD_PRELOAD 里拦到它：先挂上 `startup`/`activate` 回调，等 GTK 初始化
//! 完成后再用 `gtk_key_snooper_install` 装按键旁路器。旁路器比 hook 事件分发可靠——
//! GTK 自己的内部调用未必经过 PLT，而 snooper 是 GTK 官方留的「先看按键」入口。
//!
//! 只读程序使用 `thunarx_file_info_get_uri`（ThunarFile 实现了 ThunarxFileInfo 接口），
//! 不依赖任何未导出的 Thunar 私有符号。

use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use glib::variant::ToVariant;
use libloading::Library;
use quicklook_core::keys::{HoldLatch, KeyAction, MODIFIER_MASK, PreviewKey, classify};

type Runnable = unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char) -> c_int;
type Snoop = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> c_int;
type Callback = extern "C" fn(*mut c_void, *mut c_void);

/// `GdkEventType` 里与按键相关的两个值。
const KEY_PRESS: c_int = quicklook_core::keys::KEY_PRESS;
const KEY_RELEASE: c_int = quicklook_core::keys::KEY_RELEASE;

#[repr(C)]
struct GList {
    data: *mut c_void,
    next: *mut GList,
    prev: *mut GList,
}

struct Api {
    _libraries: Vec<Library>,
    gtk_key_snooper_install: unsafe extern "C" fn(Snoop, *mut c_void) -> u32,
    gtk_widget_get_parent: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    gtk_window_get_focus: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    gtk_window_get_type: unsafe extern "C" fn() -> usize,
    gtk_editable_get_type: unsafe extern "C" fn() -> usize,
    gtk_text_view_get_type: unsafe extern "C" fn() -> usize,
    gtk_widget_get_visible: unsafe extern "C" fn(*mut c_void) -> c_int,
    gtk_widget_get_toplevel: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    gtk_container_get_children: unsafe extern "C" fn(*mut c_void) -> *mut GList,
    gtk_window_list_toplevels: unsafe extern "C" fn() -> *mut GList,
    gtk_tree_view_get_model: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    gtk_tree_model_get_iter_first: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    gtk_tree_selection_select_iter: unsafe extern "C" fn(*mut c_void, *mut c_void),
    g_timeout_add:
        unsafe extern "C" fn(u32, extern "C" fn(*mut c_void) -> c_int, *mut c_void) -> u32,
    gtk_container_get_type: unsafe extern "C" fn() -> usize,
    gtk_tree_view_get_type: unsafe extern "C" fn() -> usize,
    gtk_tree_view_get_selection: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    gtk_tree_selection_get_selected_rows:
        unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut GList,
    gtk_tree_model_get_iter: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> c_int,
    gtk_tree_model_get_n_columns: unsafe extern "C" fn(*mut c_void) -> c_int,
    gtk_tree_model_get_column_type: unsafe extern "C" fn(*mut c_void, c_int) -> usize,
    gtk_tree_model_get_value: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut c_void),
    gtk_tree_path_free: unsafe extern "C" fn(*mut c_void),
    g_type_name: unsafe extern "C" fn(usize) -> *const c_char,
    g_type_is_a: unsafe extern "C" fn(usize, usize) -> c_int,
    g_value_get_object: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    g_value_unset: unsafe extern "C" fn(*mut c_void),
    g_list_free: unsafe extern "C" fn(*mut c_void),
    g_object_get_data: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    g_object_set_data: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void),
    g_signal_connect_data: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        c_int,
    ) -> u64,
    gdk_event_get_event_type: unsafe extern "C" fn(*mut c_void) -> c_int,
    gdk_event_get_keyval: unsafe extern "C" fn(*mut c_void, *mut u32) -> c_int,
    gdk_event_get_state: unsafe extern "C" fn(*mut c_void, *mut u32) -> c_int,
    exo_icon_view_get_type: Option<unsafe extern "C" fn() -> usize>,
    exo_icon_view_get_model: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    exo_icon_view_get_selected_items: Option<unsafe extern "C" fn(*mut c_void) -> *mut GList>,
    thunarx_file_info_get_uri: Option<unsafe extern "C" fn(*mut c_void) -> *const c_char>,
}

static API: OnceLock<Option<Api>> = OnceLock::new();
static RUN: OnceLock<Option<Runnable>> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static FOUND: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    /// 预览键当前是否处于按下状态；抬起或下一次按下都会用到。
    static PRESSED: HoldLatch = const { HoldLatch::new() };
}

impl Api {
    fn load() -> Option<Self> {
        let gtk = unsafe { Library::new("libgtk-3.so.0") }.ok()?;
        let gdk = unsafe { Library::new("libgdk-3.so.0") }.ok()?;
        let gobject = unsafe { Library::new("libgobject-2.0.so.0") }.ok()?;
        let glib = unsafe { Library::new("libglib-2.0.so.0") }.ok()?;
        let exo = unsafe { Library::new("libexo-2.so.0") }.ok();
        let thunarx = unsafe { Library::new("libthunarx-3.so.0") }.ok();

        macro_rules! required {
            ($library:expr, $name:literal, $type:ty) => {{
                let symbol = unsafe { $library.get::<$type>(concat!($name, "\0").as_bytes()) }
                    .with_context(|| format!("缺少符号 {}", $name))
                    .ok()?;
                *symbol
            }};
        }
        macro_rules! optional {
            ($library:expr, $name:literal, $type:ty) => {{
                $library
                    .as_ref()
                    .and_then(|library| {
                        unsafe { library.get::<$type>(concat!($name, "\0").as_bytes()) }.ok()
                    })
                    .map(|symbol| *symbol)
            }};
        }

        let api = Api {
            gtk_key_snooper_install: required!(gtk, "gtk_key_snooper_install", _),
            gtk_widget_get_parent: required!(gtk, "gtk_widget_get_parent", _),
            gtk_widget_get_toplevel: required!(gtk, "gtk_widget_get_toplevel", _),
            gtk_window_get_focus: required!(gtk, "gtk_window_get_focus", _),
            gtk_window_get_type: required!(gtk, "gtk_window_get_type", _),
            gtk_editable_get_type: required!(gtk, "gtk_editable_get_type", _),
            gtk_text_view_get_type: required!(gtk, "gtk_text_view_get_type", _),
            gtk_widget_get_visible: required!(gtk, "gtk_widget_get_visible", _),
            gtk_container_get_children: required!(gtk, "gtk_container_get_children", _),
            gtk_window_list_toplevels: required!(gtk, "gtk_window_list_toplevels", _),
            gtk_tree_view_get_model: required!(gtk, "gtk_tree_view_get_model", _),
            gtk_tree_model_get_iter_first: required!(gtk, "gtk_tree_model_get_iter_first", _),
            gtk_tree_selection_select_iter: required!(gtk, "gtk_tree_selection_select_iter", _),
            g_timeout_add: required!(glib, "g_timeout_add", _),
            gtk_container_get_type: required!(gtk, "gtk_container_get_type", _),
            gtk_tree_view_get_type: required!(gtk, "gtk_tree_view_get_type", _),
            gtk_tree_view_get_selection: required!(gtk, "gtk_tree_view_get_selection", _),
            gtk_tree_selection_get_selected_rows: required!(
                gtk,
                "gtk_tree_selection_get_selected_rows",
                _
            ),
            gtk_tree_model_get_iter: required!(gtk, "gtk_tree_model_get_iter", _),
            gtk_tree_model_get_n_columns: required!(gtk, "gtk_tree_model_get_n_columns", _),
            gtk_tree_model_get_column_type: required!(gtk, "gtk_tree_model_get_column_type", _),
            gtk_tree_model_get_value: required!(gtk, "gtk_tree_model_get_value", _),
            gtk_tree_path_free: required!(gtk, "gtk_tree_path_free", _),
            g_type_name: required!(gobject, "g_type_name", _),
            g_type_is_a: required!(gobject, "g_type_is_a", _),
            g_value_get_object: required!(gobject, "g_value_get_object", _),
            g_value_unset: required!(gobject, "g_value_unset", _),
            g_list_free: required!(gobject, "g_list_free", _),
            g_object_get_data: required!(gobject, "g_object_get_data", _),
            g_object_set_data: required!(gobject, "g_object_set_data", _),
            g_signal_connect_data: required!(gobject, "g_signal_connect_data", _),
            gdk_event_get_event_type: required!(gdk, "gdk_event_get_event_type", _),
            gdk_event_get_keyval: required!(gdk, "gdk_event_get_keyval", _),
            gdk_event_get_state: required!(gdk, "gdk_event_get_state", _),
            exo_icon_view_get_type: optional!(exo, "exo_icon_view_get_type", _),
            exo_icon_view_get_model: optional!(exo, "exo_icon_view_get_model", _),
            exo_icon_view_get_selected_items: optional!(exo, "exo_icon_view_get_selected_items", _),
            thunarx_file_info_get_uri: optional!(thunarx, "thunarx_file_info_get_uri", _),
            _libraries: [
                Some(gtk),
                Some(gdk),
                Some(gobject),
                Some(glib),
                exo,
                thunarx,
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        Some(api)
    }
}

fn api() -> Option<&'static Api> {
    API.get_or_init(Api::load).as_ref()
}

fn preview_key() -> &'static PreviewKey {
    static KEY: OnceLock<PreviewKey> = OnceLock::new();
    KEY.get_or_init(|| {
        quicklook_core::config::Config::load()
            .map(|config| config.thunar_key())
            .unwrap_or_default()
    })
}

/// GObject 实例的类型号：`GTypeInstance.g_class->g_type`。
fn object_type(object: *mut c_void) -> usize {
    if object.is_null() {
        return 0;
    }
    unsafe {
        let class = *(object as *mut *mut c_void);
        if class.is_null() {
            return 0;
        }
        *(class as *mut usize)
    }
}

fn type_name(api: &Api, widget: *mut c_void) -> String {
    let kind = object_type(widget);
    if kind == 0 {
        return String::new();
    }
    let name = unsafe { (api.g_type_name)(kind) };
    if name.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    }
}

fn is_tree_view(api: &Api, widget: *mut c_void) -> bool {
    let wanted = unsafe { (api.gtk_tree_view_get_type)() };
    unsafe { (api.g_type_is_a)(object_type(widget), wanted) != 0 }
}

fn is_icon_view(api: &Api, widget: *mut c_void) -> bool {
    let Some(get_type) = api.exo_icon_view_get_type else {
        return false;
    };
    let wanted = unsafe { get_type() };
    unsafe { (api.g_type_is_a)(object_type(widget), wanted) != 0 }
}

/// 只认 Thunar 自己的两个文件视图类。
///
/// 不能笼统地认「是 GtkTreeView」——侧边栏（书签）也是 GtkTreeView，而且是窗口里的
/// 第一个，按祖先链/深度优先都会先撞上它，于是读到的永远是一个空选择。
fn view_kind(api: &Api, widget: *mut c_void) -> Option<bool> {
    match type_name(api, widget).as_str() {
        "ThunarDetailsView" => Some(true),
        "ThunarIconView" => Some(false),
        _ => None,
    }
}

/// `ThunarDetailsView` / `ThunarIconView` 在 4.18 是复合容器（由 .ui 模板搭出来），
/// 真正的 `GtkTreeView` / `ExoIconView` 在它内部，所以还要往里找一层。
fn view_under(api: &Api, marker: *mut c_void, tree: bool) -> Option<*mut c_void> {
    let direct = if tree {
        is_tree_view(api, marker)
    } else {
        is_icon_view(api, marker)
    };
    if direct {
        return Some(marker);
    }
    FOUND.set(std::ptr::null_mut());
    walk_view(api, marker, tree, 0);
    let found = FOUND.get();
    if found.is_null() { None } else { Some(found) }
}

/// 找到承载文件列表的视图：焦点控件通常在列表上，先沿祖先链找 Thunar 视图标记；
/// 路径栏获得焦点时祖先链上没有标记，再在整个窗口里搜。两种情况都只认 Thunar 的
/// 视图类，避免误把侧边栏当成文件列表。
fn find_view(api: &Api, widget: *mut c_void) -> Option<(*mut c_void, bool)> {
    let top = unsafe { (api.gtk_widget_get_toplevel)(widget) };
    if top.is_null()
        || unsafe { (api.g_type_is_a)(object_type(top), (api.gtk_window_get_type)()) } == 0
    {
        return None;
    }
    let focus = unsafe { (api.gtk_window_get_focus)(top) };
    let mut node = if focus.is_null() { widget } else { focus };
    while !node.is_null() {
        if unsafe {
            (api.g_type_is_a)(object_type(node), (api.gtk_editable_get_type)()) != 0
                || (api.g_type_is_a)(object_type(node), (api.gtk_text_view_get_type)()) != 0
        } {
            return None;
        }
        if let Some(tree) = view_kind(api, node)
            && let Some(view) = view_under(api, node, tree)
        {
            return Some((view, tree));
        }
        node = unsafe { (api.gtk_widget_get_parent)(node) };
    }
    // 焦点不在文件视图里（点了侧边栏书签、工具栏按钮、状态栏等）时，退回到
    // 窗口里的第一个文件视图。否则「空格关闭」会静默失效——这是最常见的
    // 「有时候按空格没反应」。焦点在输入框/文本框里时上面已返回 None，
    // 所以不会抢走正在输入的字符。
    FOUND.set(std::ptr::null_mut());
    walk_marker(api, top, 0);
    let marker = FOUND.get();
    if marker.is_null() {
        return None;
    }
    let tree = view_kind(api, marker)?;
    view_under(api, marker, tree).map(|view| (view, tree))
}

fn children(api: &Api, widget: *mut c_void) -> Option<*mut GList> {
    let container = unsafe { (api.gtk_container_get_type)() };
    if unsafe { (api.g_type_is_a)(object_type(widget), container) } == 0 {
        return None;
    }
    let list = unsafe { (api.gtk_container_get_children)(widget) };
    if list.is_null() { None } else { Some(list) }
}

fn walk_marker(api: &Api, widget: *mut c_void, depth: usize) {
    if widget.is_null() || !FOUND.get().is_null() || depth > 12 {
        return;
    }
    if unsafe { (api.gtk_widget_get_visible)(widget) } == 0 {
        return;
    }
    if view_kind(api, widget).is_some() {
        FOUND.set(widget);
        return;
    }
    let Some(list) = children(api, widget) else {
        return;
    };
    let mut cursor = list;
    while !cursor.is_null() {
        walk_marker(api, unsafe { (*cursor).data }, depth + 1);
        if !FOUND.get().is_null() {
            break;
        }
        cursor = unsafe { (*cursor).next };
    }
    unsafe { (api.g_list_free)(list as *mut c_void) };
}

fn walk_view(api: &Api, widget: *mut c_void, tree: bool, depth: usize) {
    if widget.is_null() || !FOUND.get().is_null() || depth > 12 {
        return;
    }
    let matched = if tree {
        is_tree_view(api, widget)
    } else {
        is_icon_view(api, widget)
    };
    if matched {
        FOUND.set(widget);
        return;
    }
    let Some(list) = children(api, widget) else {
        return;
    };
    let mut cursor = list;
    while !cursor.is_null() {
        walk_view(api, unsafe { (*cursor).data }, tree, depth + 1);
        if !FOUND.get().is_null() {
            break;
        }
        cursor = unsafe { (*cursor).next };
    }
    unsafe { (api.g_list_free)(list as *mut c_void) };
}

/// 模型里承载 `ThunarFile` 的列。缩略图、名称、大小这些列都是原始类型，
/// 只有一列是对象，因此按名字找最稳，不依赖行号假死。
fn file_column(api: &Api, model: *mut c_void) -> c_int {
    if model.is_null() {
        return -1;
    }
    let columns = unsafe { (api.gtk_tree_model_get_n_columns)(model) };
    for column in 0..columns.min(32) {
        let kind = unsafe { (api.gtk_tree_model_get_column_type)(model, column) };
        let name = unsafe { (api.g_type_name)(kind) };
        if name.is_null() {
            continue;
        }
        if unsafe { CStr::from_ptr(name) }.to_string_lossy() == "ThunarFile" {
            return column;
        }
    }
    -1
}

fn uri_at(api: &Api, model: *mut c_void, iter: *mut c_void, column: c_int) -> Option<String> {
    let get_uri = api.thunarx_file_info_get_uri?;
    let mut value = [0usize; 4];
    let slot = value.as_mut_ptr() as *mut c_void;
    unsafe { (api.gtk_tree_model_get_value)(model, iter, column, slot) };
    let object = unsafe { (api.g_value_get_object)(slot) };
    if std::env::var_os("QUICKLOOK_THUNAR_DEBUG").is_some() {
        let kind = object_type(object);
        let name = unsafe { (api.g_type_name)(kind) };
        let label = if name.is_null() {
            "?".to_string()
        } else {
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        };
        log(&format!(
            "取值：列 {column} 对象={object:p} 类型={label}（列类型见上）"
        ));
    }
    let uri = if object.is_null() {
        None
    } else {
        let pointer = unsafe { get_uri(object) };
        if pointer.is_null() {
            None
        } else {
            Some(
                unsafe { CStr::from_ptr(pointer) }
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    };
    if !object.is_null() {
        unsafe { (api.g_value_unset)(slot) };
    }
    uri
}

fn rows_uris(api: &Api, model: *mut c_void, rows: *mut GList) -> Vec<String> {
    let column = file_column(api, model);
    let debug = std::env::var_os("QUICKLOOK_THUNAR_DEBUG").is_some();
    if debug {
        log(&format!(
            "读选择：模型={model:p} 行={rows:p} 文件列={column}"
        ));
    }
    if column < 0 {
        return Vec::new();
    }
    let mut uris = Vec::new();
    let mut cursor = rows;
    while !cursor.is_null() {
        let path = unsafe { (*cursor).data };
        let mut iter = [0usize; 4];
        let iter_slot = iter.as_mut_ptr() as *mut c_void;
        if unsafe { (api.gtk_tree_model_get_iter)(model, iter_slot, path) } != 0
            && let Some(uri) = uri_at(api, model, iter_slot, column)
        {
            uris.push(uri);
        }
        unsafe { (api.gtk_tree_path_free)(path) };
        cursor = unsafe { (*cursor).next };
    }
    unsafe { (api.g_list_free)(rows as *mut c_void) };
    uris
}

fn tree_uris(api: &Api, view: *mut c_void) -> Vec<String> {
    let selection = unsafe { (api.gtk_tree_view_get_selection)(view) };
    if selection.is_null() {
        return Vec::new();
    }
    let mut model: *mut c_void = std::ptr::null_mut();
    let rows = unsafe { (api.gtk_tree_selection_get_selected_rows)(selection, &mut model) };
    if std::env::var_os("QUICKLOOK_THUNAR_DEBUG").is_some() {
        log(&format!(
            "明细视图：选择={selection:p} 模型={model:p} 行={rows:p}"
        ));
    }
    if rows.is_null() {
        return Vec::new();
    }
    rows_uris(api, model, rows)
}

fn icon_uris(api: &Api, view: *mut c_void) -> Vec<String> {
    let (Some(get_model), Some(get_items)) = (
        api.exo_icon_view_get_model,
        api.exo_icon_view_get_selected_items,
    ) else {
        return Vec::new();
    };
    let model = unsafe { get_model(view) };
    let items = unsafe { get_items(view) };
    if items.is_null() {
        return Vec::new();
    }
    rows_uris(api, model, items)
}

/// 交给 broker 处理：`Toggle` 与 CLI 的默认动作一致——没开就开，开着同一个文件就关。
fn toggle(uris: Vec<String>) {
    std::thread::spawn(move || {
        let outcome = (|| -> Result<()> {
            let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
                .context("拿不到会话总线")?;
            connection
                .call_sync(
                    Some(quicklook_core::protocol::BUS),
                    quicklook_core::protocol::PATH,
                    quicklook_core::protocol::BUS,
                    "Toggle",
                    Some(&(uris,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                    None::<&gio::Cancellable>,
                )
                .context("调用 Toggle 失败")?;
            Ok(())
        })();
        if let Err(error) = outcome {
            log(&format!("调用预览服务失败：{error:#}"));
        }
    });
}

extern "C" fn reset_held(_window: *mut c_void, _event: *mut c_void, _data: *mut c_void) -> c_int {
    PRESSED.with(HoldLatch::release);
    0
}

fn watch_focus(api: &Api, widget: *mut c_void) {
    let window = unsafe { (api.gtk_widget_get_toplevel)(widget) };
    if window.is_null() {
        return;
    }
    unsafe {
        if (api.g_object_get_data)(window, c"quicklook-key-focus-hook".as_ptr()).is_null() {
            (api.g_object_set_data)(
                window,
                c"quicklook-key-focus-hook".as_ptr(),
                std::ptr::dangling_mut::<c_void>(),
            );
            // 预览窗口出现时焦点会移到它那边，宿主只收到 focus-out、收不到空格抬起。
            // 只清 focus-in 的话锁存会一直停在「按下」，下一次空格被当成自动重复吞掉，
            // 表现就是「空格要按两下才关」。
            for signal in [c"focus-in-event", c"focus-out-event"] {
                (api.g_signal_connect_data)(
                    window,
                    signal.as_ptr(),
                    reset_held as *mut c_void,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
        }
    }
}

extern "C" fn snoop(widget: *mut c_void, event: *mut c_void, _data: *mut c_void) -> c_int {
    let Some(api) = api() else {
        return 0;
    };
    if widget.is_null() || event.is_null() {
        return 0;
    }
    let mut keyval = 0u32;
    if unsafe { (api.gdk_event_get_keyval)(event, &mut keyval) } == 0 {
        return 0;
    }
    let wanted = preview_key();
    let debug = std::env::var_os("QUICKLOOK_THUNAR_DEBUG").is_some();
    let kind = unsafe { (api.gdk_event_get_event_type)(event) };
    let mut state = 0u32;
    if unsafe { (api.gdk_event_get_state)(event, &mut state) } == 0 && kind == KEY_PRESS {
        return 0;
    }
    let pressed = PRESSED.with(|latch| latch.pressed());
    let rapid = PRESSED.with(|latch| latch.rapid());
    if debug {
        log(&format!(
            "按键 kind={kind} keyval={keyval:#x} 期望={:#x} state={state:#x} 需要={:#x} 已按下={pressed}",
            wanted.keyval(),
            wanted.x11_modifiers()
        ));
    }
    match classify(kind, keyval, state, wanted, pressed) {
        KeyAction::Ignore => return 0,
        // 锁存还停在「按下」，但距上次按下已超过自动重复的间隔：用户很快又按了一下
        // （通常是想关掉预览），不能当成重复吞掉，按新的一次按下处理。
        KeyAction::Consume if kind == KEY_PRESS && !rapid => {
            if debug {
                log("疑似新的一次按下（超过自动重复间隔），按 Toggle 处理");
            }
        }
        KeyAction::Consume => {
            PRESSED.with(|latch| match kind {
                KEY_RELEASE => latch.release(),
                _ => latch.press(),
            });
            return 1;
        }
        KeyAction::Activate => {}
    }
    let found = find_view(api, widget);
    if debug {
        log(&format!(
            "命中预览键：修饰键 {:#x}，找到视图={}",
            state & MODIFIER_MASK,
            found.is_some()
        ));
    }
    let Some((view, tree)) = found else {
        return 0;
    };
    let uris = if tree {
        tree_uris(api, view)
    } else {
        icon_uris(api, view)
    };
    if uris.is_empty() {
        return 0;
    }
    watch_focus(api, widget);
    PRESSED.with(HoldLatch::press);
    log(&format!("{} 预览 {} 个文件", wanted.describe(), uris.len()));
    toggle(uris);
    1
}

/// LD_PRELOAD 会被子进程继承：Thunar 打开图片会启动 xviewer，那个进程同样会调
/// `g_application_run`。只在自己就是 Thunar 时才接管，否则会把别的 GTK 程序的空格也吃掉。
fn inside_thunar() -> bool {
    std::fs::read_link("/proc/self/exe")
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .is_some_and(|name| name == "thunar" || name == "Thunar")
}

fn install(api: &Api) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let id = unsafe { (api.gtk_key_snooper_install)(snoop, std::ptr::null_mut()) };
    if id == 0 {
        INSTALLED.store(false, Ordering::SeqCst);
        log("安装按键旁路器失败");
    } else {
        log(&format!(
            "按键旁路器已安装（id={id}），预览键 {}",
            preview_key().describe()
        ));
        if std::env::var_os("QUICKLOOK_THUNAR_DEBUG").is_some() {
            unsafe { (api.g_timeout_add)(2000, debug_tick, std::ptr::null_mut()) };
        }
    }
}

/// 自检：不按任何键，直接把第一个视图的第一行选中再读 URI，验证「找视图 + 定位
/// ThunarFile 列 + 取 URI」整条链路。只在 QUICKLOOK_THUNAR_DEBUG 下挂一次。
extern "C" fn debug_tick(_data: *mut c_void) -> c_int {
    if let Some(api) = api() {
        let windows = unsafe { (api.gtk_window_list_toplevels)() };
        let mut cursor = windows;
        let mut count = 0usize;
        while !cursor.is_null() {
            let window = unsafe { (*cursor).data };
            cursor = unsafe { (*cursor).next };
            count += 1;
            let Some((view, tree)) = find_view(api, window) else {
                log("自检：这个窗口里没有找到文件视图");
                continue;
            };
            let model = if tree {
                unsafe { (api.gtk_tree_view_get_model)(view) }
            } else {
                api.exo_icon_view_get_model
                    .map(|get| unsafe { get(view) })
                    .unwrap_or(std::ptr::null_mut())
            };
            if model.is_null() {
                log("自检：视图没有模型");
                continue;
            }
            let mut iter = [0usize; 4];
            let iter_slot = iter.as_mut_ptr() as *mut c_void;
            if unsafe { (api.gtk_tree_model_get_iter_first)(model, iter_slot) } != 0 {
                if tree {
                    let selection = unsafe { (api.gtk_tree_view_get_selection)(view) };
                    unsafe { (api.gtk_tree_selection_select_iter)(selection, iter_slot) };
                }
                let columns = unsafe { (api.gtk_tree_model_get_n_columns)(model) };
                let mut names = Vec::new();
                for column in 0..columns.min(32) {
                    let kind = unsafe { (api.gtk_tree_model_get_column_type)(model, column) };
                    let name = unsafe { (api.g_type_name)(kind) };
                    names.push(if name.is_null() {
                        "?".to_string()
                    } else {
                        unsafe { CStr::from_ptr(name) }
                            .to_string_lossy()
                            .into_owned()
                    });
                }
                log(&format!(
                    "自检：{columns} 列，类型 {:?}，uri 符号={}，文件列={}",
                    names,
                    api.thunarx_file_info_get_uri.is_some(),
                    file_column(api, model)
                ));
                let uris = if tree {
                    tree_uris(api, view)
                } else {
                    icon_uris(api, view)
                };
                log(&format!(
                    "自检：{} 选中第一行后读到 {} 个 URI：{:?}",
                    if tree { "明细视图" } else { "图标视图" },
                    uris.len(),
                    uris.iter().take(3).collect::<Vec<_>>()
                ));
            } else {
                log("自检：模型是空的");
            }
        }
        log(&format!("自检：共 {count} 个顶层窗口"));
    }
    0
}
extern "C" fn on_startup(_application: *mut c_void, _data: *mut c_void) {
    if let Some(api) = api() {
        install(api);
    }
}

extern "C" fn on_activate(_application: *mut c_void, _data: *mut c_void) {
    if let Some(api) = api() {
        install(api);
    }
}

fn real_run() -> Option<Runnable> {
    *RUN.get_or_init(|| {
        if let Ok(gio) = unsafe { Library::new("libgio-2.0.so.0") }
            && let Ok(symbol) = unsafe { gio.get::<Runnable>(b"g_application_run\0".as_slice()) }
        {
            return Some(*symbol);
        }
        unsafe {
            let pointer = libc::dlsym(libc::RTLD_NEXT, c"g_application_run".as_ptr());
            if pointer.is_null() {
                None
            } else {
                Some(std::mem::transmute::<*mut c_void, Runnable>(pointer))
            }
        }
    })
}

/// 注入点：Thunar 的 `main` 会调用它，符号解析经过 PLT，因此这里能先拿到控制权。
///
/// # Safety
/// 由动态链接器调用，参数是 GLib 约定的 `GApplication`、`argc`、`argv`。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g_application_run(
    application: *mut c_void,
    argc: c_int,
    argv: *mut *mut c_char,
) -> c_int {
    let Some(run) = real_run() else {
        log("找不到真正的 g_application_run，放弃注入");
        return 1;
    };
    if !inside_thunar() {
        return unsafe { run(application, argc, argv) };
    }
    if let Some(api) = api()
        && !application.is_null()
    {
        for signal in [c"startup", c"activate"] {
            let handler: Callback = if signal == c"startup" {
                on_startup
            } else {
                on_activate
            };
            unsafe {
                (api.g_signal_connect_data)(
                    application,
                    signal.as_ptr(),
                    handler as *mut c_void,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                )
            };
        }
    }
    unsafe { run(application, argc, argv) }
}

fn log_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("quicklook").join("thunar-adapter.log")
}

/// 适配器跑在别人的进程里，出问题只能靠日志回溯，所以固定留一份小的。
fn log(message: &str) {
    use std::io::Write;
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(&path)
        .map(|meta| meta.len() > 65536)
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{} {message}", unsafe { libc::getpid() });
    }
}
