//! Caja 适配：由宿主扩展加载，或通过 LD_PRELOAD 加载，把预览键交给 Quick Look。
//!
//! Caja 与 Nemo 同源，但发行版构建的 `/usr/bin/caja` 把内部函数都留在了动态符号表里
//! （`caja_view_get_type`、`caja_view_get_selection` 都是 `T` 符号），所以这里按名字
//! `dlsym` 就够用，不必像 Nemo 适配器那样按 vtable 偏移猜函数位置。
//!
//! `caja_view_get_selection` 交回的链表元素类型随版本而变：本机 Caja 1.26 给的是
//! `GFile`（`GLocalFile`），不是 `CajaFile`。所以逐个按 GObject 实际类型选取 URI 的
//! 函数，类型对不上就什么都不调 —— 直接拿 `caja_file_info_get_uri` 去读 `GFile`
//! 只会刷一屏 `CAJA_IS_FILE_INFO` critical，然后静默交出空列表。
//!
//! 只要有一个关键符号拿不到就完全不接管按键：宁可空格维持 Caja 原样，也不能吞掉按键却
//! 读不到选中项——那会让用户的文件管理器看起来「按了没反应」。

use anyhow::{Context, Result};
use glib::variant::ToVariant;
use libloading::Library;
use quicklook_core::keys::{HoldLatch, KeyAction, PreviewKey, classify};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

type Runnable = unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char) -> c_int;
type Snoop = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> c_int;
type Callback = extern "C" fn(*mut c_void, *mut c_void);
type GetType = unsafe extern "C" fn() -> usize;
type GetSelection = unsafe extern "C" fn(*mut c_void) -> *mut GList;
type GetUri = unsafe extern "C" fn(*mut c_void) -> *mut c_char;
type Unref = unsafe extern "C" fn(*mut c_void);

/// GTK3 事件类型：按下与抬起都会进 snooper，判定在 `quicklook_core::keys::classify`。
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
    key_snooper_install: unsafe extern "C" fn(Snoop, *mut c_void) -> u32,
    settings_get_default: unsafe extern "C" fn() -> *mut c_void,
    widget_get_parent: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    widget_get_toplevel: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    widget_get_visible: unsafe extern "C" fn(*mut c_void) -> c_int,
    container_get_children: unsafe extern "C" fn(*mut c_void) -> *mut GList,
    container_get_type: GetType,
    window_get_type: GetType,
    editable_get_type: GetType,
    text_view_get_type: GetType,
    window_get_focus: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    g_type_is_a: unsafe extern "C" fn(usize, usize) -> c_int,
    g_type_name: unsafe extern "C" fn(usize) -> *const c_char,
    g_free: unsafe extern "C" fn(*mut c_void),
    g_list_free: unsafe extern "C" fn(*mut c_void),
    g_object_unref: Unref,
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
    event_keyval: unsafe extern "C" fn(*mut c_void, *mut u32) -> c_int,
    event_state: unsafe extern "C" fn(*mut c_void, *mut u32) -> c_int,
    event_type: unsafe extern "C" fn(*mut c_void) -> c_int,
    /// 选中项的 URI 按元素实际类型取：新版给 `GFile`，老构建可能给 `CajaFile`
    /// 或扩展层的 `CajaFileInfo`。
    g_file_get_type: GetType,
    g_file_get_uri: GetUri,
    file_get_type: Option<GetType>,
    file_info_get_type: Option<GetType>,
    file_info_get_uri: Option<GetUri>,
    file_get_uri: Option<GetUri>,
    /// 下面两个来自 Caja 主程序自己的动态符号表。
    view_get_type: GetType,
    view_get_selection: GetSelection,
}

impl Api {
    fn load() -> Option<Self> {
        let gtk = unsafe { Library::new("libgtk-3.so.0") }.ok()?;
        let gdk = unsafe { Library::new("libgdk-3.so.0") }.ok()?;
        let gobject = unsafe { Library::new("libgobject-2.0.so.0") }.ok()?;
        let glib = unsafe { Library::new("libglib-2.0.so.0") }.ok()?;
        let gio = unsafe { Library::new("libgio-2.0.so.0") }.ok()?;
        let extension = unsafe { Library::new("libcaja-extension.so.1") }.ok();

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
            key_snooper_install: required!(gtk, "gtk_key_snooper_install", _),
            settings_get_default: required!(gtk, "gtk_settings_get_default", _),
            widget_get_parent: required!(gtk, "gtk_widget_get_parent", _),
            widget_get_toplevel: required!(gtk, "gtk_widget_get_toplevel", _),
            widget_get_visible: required!(gtk, "gtk_widget_get_visible", _),
            container_get_children: required!(gtk, "gtk_container_get_children", _),
            container_get_type: required!(gtk, "gtk_container_get_type", _),
            window_get_type: required!(gtk, "gtk_window_get_type", _),
            editable_get_type: required!(gtk, "gtk_editable_get_type", _),
            text_view_get_type: required!(gtk, "gtk_text_view_get_type", _),
            window_get_focus: required!(gtk, "gtk_window_get_focus", _),
            g_type_is_a: required!(gobject, "g_type_is_a", _),
            g_type_name: required!(gobject, "g_type_name", _),
            g_free: required!(glib, "g_free", _),
            g_list_free: required!(glib, "g_list_free", _),
            g_object_unref: required!(gobject, "g_object_unref", _),
            g_object_get_data: required!(gobject, "g_object_get_data", _),
            g_object_set_data: required!(gobject, "g_object_set_data", _),
            g_signal_connect_data: required!(gobject, "g_signal_connect_data", _),
            event_keyval: required!(gdk, "gdk_event_get_keyval", _),
            event_state: required!(gdk, "gdk_event_get_state", _),
            event_type: required!(gdk, "gdk_event_get_event_type", _),
            g_file_get_type: required!(gio, "g_file_get_type", _),
            g_file_get_uri: required!(gio, "g_file_get_uri", _),
            file_get_type: binary_symbol(c"caja_file_get_type"),
            file_info_get_type: optional!(extension, "caja_file_info_get_type", _),
            file_info_get_uri: optional!(extension, "caja_file_info_get_uri", _),
            file_get_uri: binary_symbol(c"caja_file_get_uri"),
            view_get_type: binary_symbol(c"caja_view_get_type")?,
            view_get_selection: binary_symbol(c"caja_view_get_selection")?,
            _libraries: [
                Some(gtk),
                Some(gdk),
                Some(gobject),
                Some(glib),
                Some(gio),
                extension,
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        Some(api)
    }
}

/// 从进程的全局符号表里按名字取符号：Caja 主程序导出自己的内部函数，
/// 所以不需要 dlopen 某个路径，直接在已加载镜像里找。
fn binary_symbol<T: Copy>(name: &CStr) -> Option<T> {
    let pointer = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    if pointer.is_null() {
        return None;
    }
    // 取到的就是符号地址本身；两边都是 8 字节，按目标函数指针类型重解读。
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&pointer) })
}

static API: OnceLock<Option<Api>> = OnceLock::new();
static RUN: OnceLock<Option<Runnable>> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static FOUND: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    /// 预览键当前是否处于按下状态；抬起或下一次按下都会用到。
    static PRESSED: HoldLatch = const { HoldLatch::new() };
}

fn api() -> Option<&'static Api> {
    API.get_or_init(Api::load).as_ref()
}

fn preview_key() -> &'static PreviewKey {
    static KEY: OnceLock<PreviewKey> = OnceLock::new();
    KEY.get_or_init(|| {
        quicklook_core::config::Config::load()
            .map(|config| config.caja_key())
            .unwrap_or_default()
    })
}

fn debug_on() -> bool {
    std::env::var_os("QUICKLOOK_CAJA_DEBUG").is_some()
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

fn is_a(api: &Api, widget: *mut c_void, wanted: GetType) -> bool {
    let kind = unsafe { wanted() };
    kind != 0 && unsafe { (api.g_type_is_a)(object_type(widget), kind) != 0 }
}

/// 可选类型的判定：符号没拿到就当不是，避免对陌生类型调它的函数。
fn is_kind(api: &Api, widget: *mut c_void, wanted: Option<GetType>) -> bool {
    wanted.is_some_and(|wanted| is_a(api, widget, wanted))
}

fn is_view(api: &Api, widget: *mut c_void) -> bool {
    is_a(api, widget, api.view_get_type)
}

/// 焦点控件通常就在文件视图里，沿祖先链找最准：Caja 的图标视图、列表视图、紧凑视图
/// 都是 `CajaView` 的子类，而侧栏、路径栏、标签页控件不是。
fn view_from_ancestors(api: &Api, widget: *mut c_void) -> Option<*mut c_void> {
    let mut node = widget;
    while !node.is_null() {
        if is_a(api, node, api.editable_get_type) || is_a(api, node, api.text_view_get_type) {
            return None;
        }
        if is_view(api, node) {
            return Some(node);
        }
        node = unsafe { (api.widget_get_parent)(node) };
    }
    None
}

/// 焦点是否落在可输入控件（或它的子控件）上。
fn inside_text_input(api: &Api, widget: *mut c_void) -> bool {
    let mut node = widget;
    while !node.is_null() {
        if is_a(api, node, api.editable_get_type) || is_a(api, node, api.text_view_get_type) {
            return true;
        }
        node = unsafe { (api.widget_get_parent)(node) };
    }
    false
}

fn children(api: &Api, widget: *mut c_void) -> Option<*mut GList> {
    if !is_a(api, widget, api.container_get_type) {
        return None;
    }
    let list = unsafe { (api.container_get_children)(widget) };
    if list.is_null() { None } else { Some(list) }
}

/// 一个窗口里可以同时挂着多个页签的视图，隐藏的页签也在容器子节点里；
/// 只往下走可见分支，才不会把预览开在用户看不到的那个页签上。
fn scan(api: &Api, widget: *mut c_void, depth: usize) {
    if widget.is_null() || !FOUND.get().is_null() || depth > 12 {
        return;
    }
    if unsafe { (api.widget_get_visible)(widget) } == 0 {
        return;
    }
    if is_view(api, widget) {
        FOUND.set(widget);
        return;
    }
    let Some(list) = children(api, widget) else {
        return;
    };
    let mut cursor = list;
    while !cursor.is_null() {
        scan(api, unsafe { (*cursor).data }, depth + 1);
        if !FOUND.get().is_null() {
            break;
        }
        cursor = unsafe { (*cursor).next };
    }
    unsafe { (api.g_list_free)(list as *mut c_void) };
}

/// 找到该用哪个视图：事件控件的祖先链 → 窗口焦点控件的祖先链 → 整个窗口的可见子节点。
fn find_view(api: &Api, widget: *mut c_void) -> Option<*mut c_void> {
    let toplevel = unsafe { (api.widget_get_toplevel)(widget) };
    if toplevel.is_null() || !is_a(api, toplevel, api.window_get_type) {
        return None;
    }
    let focus = unsafe { (api.window_get_focus)(toplevel) };
    if !focus.is_null() {
        if let Some(view) = view_from_ancestors(api, focus) {
            return Some(view);
        }
        // 焦点在地址栏、搜索框或行内重命名里就放弃，别抢走正在输入的字符。
        if inside_text_input(api, focus) {
            return None;
        }
    } else if let Some(view) = view_from_ancestors(api, widget) {
        return Some(view);
    }
    // 焦点在侧边栏、工具栏或状态栏上：退回到窗口里的文件视图，让空格仍然
    // 作用于当前选择。否则「空格关闭」会静默失效。
    FOUND.set(std::ptr::null_mut());
    scan(api, toplevel, 0);
    let found = FOUND.get();
    if found.is_null() { None } else { Some(found) }
}

/// 按元素自己的 GObject 类型取 URI。类型对不上就不调函数：Caja 的这些取 URI 接口
/// 都带 `g_return_val_if_fail`，调错类型只会打一屏 critical 再返回 NULL。
fn file_uri(api: &Api, file: *mut c_void) -> Option<String> {
    let pointer = if is_kind(api, file, Some(api.g_file_get_type)) {
        unsafe { (api.g_file_get_uri)(file) }
    } else if let (true, Some(get_uri)) = (is_kind(api, file, api.file_get_type), api.file_get_uri)
    {
        unsafe { get_uri(file) }
    } else if let (true, Some(get_uri)) = (
        is_kind(api, file, api.file_info_get_type),
        api.file_info_get_uri,
    ) {
        unsafe { get_uri(file) }
    } else {
        return None;
    };
    if pointer.is_null() {
        return None;
    }
    let uri = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    // 三个取 URI 的接口都返回新分配的字符串。
    unsafe { (api.g_free)(pointer as *mut c_void) };
    Some(uri)
}

/// 读选中的文件。`caja_view_get_selection` 交回的链表每个元素都带一份引用
/// （`GFile` 是现取的，`CajaFile` 是 ref 过的），所以逐个 `g_object_unref`
/// 再 `g_list_free`；只释放链表会漏掉全部元素。
fn selection_uris(api: &Api, view: *mut c_void) -> Vec<String> {
    let list = unsafe { (api.view_get_selection)(view) };
    if list.is_null() {
        return Vec::new();
    }
    let mut uris = Vec::new();
    let mut cursor = list;
    while !cursor.is_null() {
        let file = unsafe { (*cursor).data };
        if !file.is_null() {
            match file_uri(api, file) {
                Some(uri) => uris.push(uri),
                None => {
                    if debug_on() {
                        log(&format!(
                            "认不出的选中项类型={}，跳过",
                            type_name(api, file)
                        ));
                    }
                }
            }
            unsafe { (api.g_object_unref)(file) };
        }
        cursor = unsafe { (*cursor).next };
    }
    unsafe { (api.g_list_free)(list as *mut c_void) };
    uris
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
    let window = unsafe { (api.widget_get_toplevel)(widget) };
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
    if unsafe { (api.event_keyval)(event, &mut keyval) } == 0 {
        return 0;
    }
    let wanted = preview_key();
    let kind = unsafe { (api.event_type)(event) };
    let mut state = 0u32;
    if unsafe { (api.event_state)(event, &mut state) } == 0 && kind == KEY_PRESS {
        return 0;
    }
    let pressed = PRESSED.with(|latch| latch.pressed());
    let rapid = PRESSED.with(|latch| latch.rapid());
    if debug_on() {
        log(&format!(
            "按键 kind={kind} keyval={keyval:#x} 期望={:#x} state={state:#x} 已按下={pressed} 控件={}",
            wanted.keyval(),
            type_name(api, widget)
        ));
    }
    match classify(kind, keyval, state, wanted, pressed) {
        KeyAction::Ignore => return 0,
        // 同 Thunar：超过自动重复间隔的「按下」是新的按键，不能吞。
        KeyAction::Consume if kind == KEY_PRESS && !rapid => {
            if debug_on() {
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
    let Some(view) = find_view(api, widget) else {
        // 焦点在地址栏、搜索框这类不属于视图的控件上：交回 Caja，别吞掉按键。
        if debug_on() {
            log("没有找到文件视图，交回宿主");
        }
        return 0;
    };
    let uris = selection_uris(api, view);
    if uris.is_empty() {
        if debug_on() {
            log(&format!(
                "视图 {} 的选中项为空，交回宿主",
                type_name(api, view)
            ));
        }
        return 0;
    }
    watch_focus(api, widget);
    PRESSED.with(HoldLatch::press);
    if debug_on() {
        log(&format!(
            "命中视图 {}，选中 {} 项",
            type_name(api, view),
            uris.len()
        ));
    }
    log(&format!("{} 预览 {} 个文件", wanted.describe(), uris.len()));
    toggle(uris);
    1
}

/// LD_PRELOAD 会被子进程继承：Caja 打开文件会启动编辑器、归档管理器，
/// 那些进程同样会调 `g_application_run`，但它们不该被接管。
fn inside_caja() -> bool {
    std::fs::read_link("/proc/self/exe")
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .is_some_and(|name| is_caja_program(&name))
}

/// 只有主程序 `caja` 算宿主。`caja-sendto`、`caja-open-terminal`、
/// `caja-autorun-software` 都链接了 libcaja-extension，被注入后会误以为自己能接管。
fn is_caja_program(name: &str) -> bool {
    name == "caja"
}

fn install(api: &Api) {
    let settings = unsafe { (api.settings_get_default)() };
    if settings.is_null() {
        return;
    }
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    // An extension and a preload (or two paths to the library) must not install
    // two callbacks. GtkSettings is shared across copies of this Rust library.
    let marker = c"quicklook-caja-snooper-installed";
    if !unsafe { (api.g_object_get_data)(settings, marker.as_ptr()) }.is_null() {
        return;
    }
    let id = unsafe { (api.key_snooper_install)(snoop, std::ptr::null_mut()) };
    if id == 0 {
        INSTALLED.store(false, Ordering::SeqCst);
        log("安装按键旁路器失败");
    } else {
        unsafe {
            (api.g_object_set_data)(
                settings,
                marker.as_ptr(),
                std::ptr::dangling_mut::<c_void>(),
            )
        };
        log(&format!(
            "按键旁路器已安装（id={id}），预览键 {}",
            preview_key().describe()
        ));
    }
}

/// Entry point for the user-level Caja extension, after GTK initialization.
///
/// # Safety
/// Call only from Caja's GTK main thread. Returns 0 outside Caja or when takeover
/// is disabled; no borrowed pointers cross the extension boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quicklook_caja_install() -> c_int {
    if !inside_caja()
        || !quicklook_core::config::Config::load()
            .is_ok_and(|config| config.host_caja && config.hijack_native)
    {
        return 0;
    }
    let Some(api) = api() else {
        log("Caja 扩展无法加载按键接口，保留宿主原有行为");
        return 0;
    };
    install(api);
    c_int::from(INSTALLED.load(Ordering::SeqCst))
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
        binary_symbol(c"g_application_run")
    })
}

/// 注入点：Caja 的 `main` 会调用它，符号解析经过 PLT，因此这里能先拿到控制权。
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
    if !inside_caja() {
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
    base.join("quicklook").join("caja-adapter.log")
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

#[cfg(test)]
mod tests {
    use super::is_caja_program;

    #[test]
    fn only_the_main_binary_is_hooked() {
        assert!(is_caja_program("caja"));
        // 这几个都链接了 libcaja-extension，继承注入后不能接管它们的按键。
        assert!(!is_caja_program("caja-sendto"));
        assert!(!is_caja_program("caja-open-terminal"));
        assert!(!is_caja_program("caja-autorun-software"));
        assert!(!is_caja_program("pluma"));
    }

    #[test]
    fn extension_entry_does_not_initialize_gtk_in_other_programs() {
        assert_eq!(unsafe { super::quicklook_caja_install() }, 0);
    }
}
