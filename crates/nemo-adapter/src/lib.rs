//! Nemo 6.6.3 adapter. No keyboard hooks outside Nemo's own file views.
//!
//! Nemo's private functions are not exported. Its documented source class layout
//! places get_selection at slot 11 after GtkScrolledWindowClass. The launcher
//! pins the exact Nemo executable fingerprint; an upgrade disables this path.
use glib::{ffi as gf, gobject_ffi as go, variant::ToVariant};
use quicklook_core::protocol::*;
use std::{
    cell::RefCell,
    ffi::{CStr, c_char, c_int, c_void},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
type Pointer = *mut c_void;
type Snooper = unsafe extern "C" fn(Pointer, Pointer, Pointer) -> c_int;
#[repr(C)]
struct TreeIter {
    stamp: c_int,
    data: [Pointer; 3],
}
struct Api {
    _gtk: libloading::Library,
    _gdk: libloading::Library,
    _nemo: libloading::Library,
    windows: unsafe extern "C" fn() -> *mut gf::GList,
    focus: unsafe extern "C" fn(Pointer) -> Pointer,
    toplevel: unsafe extern "C" fn(Pointer) -> Pointer,
    parent: unsafe extern "C" fn(Pointer) -> Pointer,
    active: unsafe extern "C" fn(Pointer) -> c_int,
    event_key: unsafe extern "C" fn(Pointer, *mut u32) -> c_int,
    event_state: unsafe extern "C" fn(Pointer, *mut u32) -> c_int,
    event_type: unsafe extern "C" fn(Pointer) -> c_int,
    snooper_install: unsafe extern "C" fn(Snooper, Pointer) -> u32,
    file_uri: unsafe extern "C" fn(Pointer) -> *mut c_char,
    file_type: unsafe extern "C" fn() -> usize,
    tree_model: unsafe extern "C" fn(Pointer) -> Pointer,
    tree_selection: unsafe extern "C" fn(Pointer) -> Pointer,
    unselect_all: unsafe extern "C" fn(Pointer),
    select_iter: unsafe extern "C" fn(Pointer, *mut TreeIter),
    first: unsafe extern "C" fn(Pointer, *mut TreeIter) -> c_int,
    next: unsafe extern "C" fn(Pointer, *mut TreeIter) -> c_int,
    value: unsafe extern "C" fn(Pointer, *mut TreeIter, c_int, *mut go::GValue),
}
impl Api {
    unsafe fn load() -> anyhow::Result<Self> {
        unsafe {
            let gtk = libloading::Library::new("libgtk-3.so.0")?;
            let gdk = libloading::Library::new("libgdk-3.so.0")?;
            let nemo = libloading::Library::new("libnemo-extension.so.1")?;
            Ok(Self {
                windows: *gtk.get(b"gtk_window_list_toplevels\0")?,
                focus: *gtk.get(b"gtk_window_get_focus\0")?,
                toplevel: *gtk.get(b"gtk_widget_get_toplevel\0")?,
                parent: *gtk.get(b"gtk_widget_get_parent\0")?,
                active: *gtk.get(b"gtk_window_is_active\0")?,
                event_key: *gdk.get(b"gdk_event_get_keyval\0")?,
                event_state: *gdk.get(b"gdk_event_get_state\0")?,
                event_type: *gdk.get(b"gdk_event_get_event_type\0")?,
                snooper_install: *gtk.get(b"gtk_key_snooper_install\0")?,
                file_uri: *nemo.get(b"nemo_file_info_get_uri\0")?,
                file_type: *nemo.get(b"nemo_file_info_get_type\0")?,
                tree_model: *gtk.get(b"gtk_tree_view_get_model\0")?,
                tree_selection: *gtk.get(b"gtk_tree_view_get_selection\0")?,
                unselect_all: *gtk.get(b"gtk_tree_selection_unselect_all\0")?,
                select_iter: *gtk.get(b"gtk_tree_selection_select_iter\0")?,
                first: *gtk.get(b"gtk_tree_model_get_iter_first\0")?,
                next: *gtk.get(b"gtk_tree_model_iter_next\0")?,
                value: *gtk.get(b"gtk_tree_model_get_value\0")?,
                _gtk: gtk,
                _gdk: gdk,
                _nemo: nemo,
            })
        }
    }
}
static API: OnceLock<Option<Api>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
static SNOOPER: AtomicBool = AtomicBool::new(false);
#[derive(Default)]
struct State {
    connection: Option<gio::DBusConnection>,
    visible: bool,
    pinned: bool,
    inflight: bool,
    last: Vec<String>,
    diagnostics: bool,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}

fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, &b| {
        (hash ^ b as u64).wrapping_mul(0x100000001b3)
    })
}
extern "C" fn initialize() {
    let _ = std::panic::catch_unwind(|| {
        let Ok(exe) = std::fs::read_link("/proc/self/exe") else {
            return;
        };
        if exe.file_name() != Some(std::ffi::OsStr::new("nemo")) {
            return;
        }
        let Ok(expected) = std::env::var("QUICKLOOK_NEMO_FINGERPRINT") else {
            return;
        };
        let Ok(bytes) = std::fs::read(&exe) else {
            return;
        };
        if !cfg!(target_arch = "x86_64")
            || fingerprint(&bytes) != quicklook_core::hosts::VERIFIED_NEMO_FINGERPRINT
            || format!("{:016x}", fingerprint(&bytes)) != expected
        {
            eprintln!(
                "Quick Look: Nemo build is unverified; shortcut adapter disabled (native Space service remains available)."
            );
            return;
        }
        if STARTED.swap(true, Ordering::AcqRel) {
            return;
        }
        if API
            .get_or_init(|| {
                unsafe { Api::load() }
                    .map_err(|e| eprintln!("Quick Look adapter: {e}"))
                    .ok()
            })
            .is_none()
        {
            return;
        }
        glib::timeout_add_local(Duration::from_millis(180), || {
            let _ = std::panic::catch_unwind(scan);
            glib::ControlFlow::Continue
        });
        eprintln!("Quick Look: Rust Nemo adapter enabled (pinned host binary).");
    });
}
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZER: extern "C" fn() = initialize;

fn connection() -> Option<gio::DBusConnection> {
    STATE.with(|s| {
        if s.borrow().connection.is_none() {
            let connection = if let Ok(address) = std::env::var("QUICKLOOK_DBUS_ADDRESS") {
                gio::DBusConnection::for_address_sync(
                    &address,
                    gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                        | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                    None,
                    None::<&gio::Cancellable>,
                )
            } else {
                gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
            };
            s.borrow_mut().connection = connection.ok();
        }
        s.borrow().connection.clone()
    })
}
fn send(method: &str, parameters: &glib::Variant) {
    if let Some(connection) = connection() {
        connection.call(
            Some(BUS),
            PATH,
            BUS,
            method,
            Some(parameters),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            None::<&gio::Cancellable>,
            |result| {
                if let Err(e) = result {
                    eprintln!("Quick Look Nemo request: {e}");
                }
            },
        );
    }
}
unsafe fn is_a(widget: Pointer, name: &CStr) -> bool {
    unsafe {
        let type_ = go::g_type_from_name(name.as_ptr());
        !widget.is_null() && type_ != 0 && go::g_type_check_instance_is_a(widget.cast(), type_) != 0
    }
}
fn view_from(mut widget: Pointer) -> Option<Pointer> {
    let api = API.get()?.as_ref()?;
    unsafe {
        for _ in 0..32 {
            if widget.is_null() {
                break;
            }
            if is_a(widget, c"GtkEditable") || is_a(widget, c"GtkTextView") {
                return None;
            }
            if is_a(widget, c"NemoView") {
                return Some(widget);
            }
            widget = (api.parent)(widget);
        }
    }
    None
}
fn view_for_window(window: Pointer) -> Option<Pointer> {
    let api = API.get()?.as_ref()?;
    unsafe { view_from((api.focus)(window)) }
}
fn view_for_widget(widget: Pointer) -> Option<Pointer> {
    view_from(widget)
}
fn selection_for(view: Pointer) -> Vec<String> {
    let Some(api) = API.get().and_then(Option::as_ref) else {
        return Vec::new();
    };
    unsafe {
        let parent = go::g_type_parent(go::g_type_from_name(c"NemoView".as_ptr()));
        let mut query: go::GTypeQuery = std::mem::zeroed();
        go::g_type_query(parent, &mut query);
        let offset = query.class_size as usize + 11 * std::mem::size_of::<Pointer>();
        let class = (*(view.cast::<go::GTypeInstance>())).g_class;
        let mut actual: go::GTypeQuery = std::mem::zeroed();
        go::g_type_query((*class).g_type, &mut actual);
        if offset + std::mem::size_of::<Pointer>() > actual.class_size as usize {
            return Vec::new();
        }
        let pointer = *(class.cast::<u8>().add(offset).cast::<Pointer>());
        if pointer.is_null() {
            return Vec::new();
        }
        let get: unsafe extern "C" fn(Pointer) -> *mut gf::GList = std::mem::transmute(pointer);
        let list = get(view);
        let mut current = list;
        let mut uris = Vec::new();
        let file_type = (api.file_type)();
        while !current.is_null() && uris.len() < 4096 {
            let file = (*current).data;
            if go::g_type_check_instance_is_a(file.cast(), file_type) != 0 {
                let uri = (api.file_uri)(file);
                if !uri.is_null() {
                    uris.push(CStr::from_ptr(uri).to_string_lossy().into_owned());
                    gf::g_free(uri.cast());
                }
            }
            current = (*current).next;
        }
        gf::g_list_free_full(list, Some(unref));
        uris
    }
}
fn selection(window: Pointer) -> Vec<String> {
    view_for_window(window)
        .map(selection_for)
        .unwrap_or_default()
}
unsafe extern "C" fn unref(object: Pointer) {
    if !object.is_null() {
        unsafe {
            go::g_object_unref(object.cast());
        }
    }
}
fn own_windows() -> Vec<Pointer> {
    let Some(api) = API.get().and_then(Option::as_ref) else {
        return Vec::new();
    };
    unsafe {
        let list = (api.windows)();
        let mut cursor = list;
        let mut out = Vec::new();
        while !cursor.is_null() {
            let window = (*cursor).data;
            if is_a(window, c"NemoWindow") {
                out.push(window);
            }
            cursor = (*cursor).next;
        }
        gf::g_list_free(list);
        out
    }
}
unsafe extern "C" fn reset_held(_window: Pointer, _event: Pointer, _: Pointer) -> c_int {
    HELD_KEY.with(|held| held.set(None));
    0
}
fn scan() {
    let Some(api) = API.get().and_then(Option::as_ref) else {
        return;
    };
    install_snooper();
    diagnostics();
    for window in own_windows() {
        unsafe {
            if go::g_object_get_data(window.cast(), c"quicklook-hooked".as_ptr()).is_null() {
                go::g_object_set_data(
                    window.cast(),
                    c"quicklook-hooked".as_ptr(),
                    1usize as Pointer,
                );
                for signal in [c"focus-in-event", c"focus-out-event"] {
                    go::g_signal_connect_data(
                        window.cast(),
                        signal.as_ptr(),
                        Some(std::mem::transmute::<
                            unsafe extern "C" fn(Pointer, Pointer, Pointer) -> c_int,
                            unsafe extern "C" fn(),
                        >(reset_held)),
                        std::ptr::null_mut(),
                        None,
                        0,
                    );
                }
                go::g_signal_connect_data(
                    window.cast(),
                    c"key-press-event".as_ptr(),
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(Pointer, Pointer, Pointer) -> c_int,
                        unsafe extern "C" fn(),
                    >(key_press)),
                    std::ptr::null_mut(),
                    None,
                    0,
                );
                go::g_signal_connect_data(
                    window.cast(),
                    c"key-release-event".as_ptr(),
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(Pointer, Pointer, Pointer) -> c_int,
                        unsafe extern "C" fn(),
                    >(key_release)),
                    std::ptr::null_mut(),
                    None,
                    0,
                );
            }
        }
        if unsafe { (api.active)(window) } != 0
            && STATE.with(|s| s.borrow().visible && !s.borrow().pinned)
        {
            sync_selection(window);
        }
    }
    let poll = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.inflight {
            false
        } else {
            s.inflight = true;
            true
        }
    });
    if !poll {
        return;
    }
    if let Some(connection) = connection() {
        connection.call(
            Some(BUS),
            PATH,
            BUS,
            "Status",
            None,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            500,
            None::<&gio::Cancellable>,
            |result| {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.inflight = false;
                    let status = result
                        .ok()
                        .and_then(|v| v.get::<(String,)>().map(|t| t.0))
                        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
                    s.visible = status
                        .as_ref()
                        .and_then(|v| v["visible"].as_bool())
                        .unwrap_or(false);
                    s.pinned = status
                        .as_ref()
                        .and_then(|v| v["pinned"].as_bool())
                        .unwrap_or(false);
                });
            },
        );
    } else {
        STATE.with(|s| s.borrow_mut().inflight = false);
    }
}
fn sync_selection(window: Pointer) {
    let uris = selection(window);
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.visible || s.pinned || s.last == uris {
            false
        } else {
            s.last = uris.clone();
            true
        }
    });
    if changed {
        send("SetSelection", &(uris,).to_variant());
    }
}
/// 预览键只在进程内读一次配置，避免每次按键都碰磁盘。
fn preview_key() -> quicklook_core::keys::PreviewKey {
    PREVIEW_KEY.with(|cell| {
        cell.get_or_init(|| {
            quicklook_core::config::Config::load()
                .map(|config| config.nemo_key())
                .unwrap_or_default()
        })
        .clone()
    })
}
thread_local! {
    static PREVIEW_KEY: std::cell::OnceCell<quicklook_core::keys::PreviewKey> =
        const { std::cell::OnceCell::new() };
    /// GDK 对按住不放的键会持续补发 KEY_PRESS（自动重复）。不挡住的话，
    /// 按住空格会连续 Toggle：窗口刚出现就被下一次重复按键关掉。
    ///
    /// 带时间戳是为了兜底：预览窗口抢走焦点后抬起事件会打到预览窗口，这里再也
    /// 收不到。没有兜底时锁存永远停在「按下」，下一次空格被当成自动重复吞掉。
    static HELD_KEY: std::cell::Cell<Option<(u32, std::time::Instant)>> =
        const { std::cell::Cell::new(None) };
}
/// 只有距上次按下不足自动重复间隔时才算重复；更长就是用户新按的一下。
/// 详见 `quicklook_core::keys::REPEAT_GAP`（宿主在预览窗口抢焦点前还会收到第二下）。
fn is_repeat(key: u32) -> bool {
    HELD_KEY.with(|held| {
        held.get().is_some_and(|(held, at)| {
            held == key && at.elapsed() < quicklook_core::keys::REPEAT_GAP
        })
    })
}
fn mark_held(key: u32) {
    HELD_KEY.with(|held| held.set(Some((key, std::time::Instant::now()))));
}
fn release_key(key: u32) {
    HELD_KEY.with(|held| {
        if held.get().is_some_and(|(held, _)| held == key) {
            held.set(None);
        }
    });
}
fn process_key(window: Pointer, key: u32, modifiers: u32) -> bool {
    let Some(view) = view_for_window(window) else {
        return false;
    };
    process_key_view(view, key, modifiers)
}
fn process_key_view(view: Pointer, key: u32, modifiers: u32) -> bool {
    let wanted = preview_key();
    // GDK 修饰位：SHIFT=1，CONTROL=4，MOD1=8，MOD4=64，SUPER 变体=0x4000000。
    let pressed = modifiers & (1 | 4 | 8 | 64 | 0x4000000);
    // 分离是独立组合键（默认 Ctrl+D），只要预览可见就生效，与预览键无关。
    if pressed == 4 && matches!(key, 100 | 68) && STATE.with(|s| s.borrow().visible) {
        send("Action", &("detach",).to_variant());
        return true;
    }
    if pressed & !wanted.x11_modifiers() != 0 {
        return false;
    }
    if key == wanted.keyval() && pressed & wanted.x11_modifiers() == wanted.x11_modifiers() {
        let uris = selection_for(view);
        if uris.is_empty() {
            return false;
        }
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.visible = !(s.visible && s.last == uris);
            s.last = uris.clone();
        });
        send("Toggle", &(uris,).to_variant());
        return true;
    }
    if !STATE.with(|s| s.borrow().visible) {
        return false;
    }
    if matches!(key, 0xff0d | 0xff8d) {
        if !STATE.with(|s| s.borrow().pinned) {
            STATE.with(|s| s.borrow_mut().visible = false);
            send("Close", &().to_variant());
        }
        return false;
    }
    if key == 0xff1b {
        STATE.with(|s| s.borrow_mut().visible = false);
        send("Close", &().to_variant());
        return true;
    }
    // 包内浏览：Backspace 退回上一层。
    if key == 0xff08 {
        send("Action", &("back".to_string(),).to_variant());
        return true;
    }
    let action = match key {
        102 | 70 => "fullscreen",
        112 | 80 => "pin",
        114 => "rotate",
        82 => "rotate-left",
        48 => "fit",
        49 => "actual",
        107 | 75 => "pause",
        105 | 73 => "properties",
        119 | 87 => "wrap",
        101 | 69 => "encoding",
        116 | 84 => "thumbnails",
        0xff55 => "page-prev",
        0xff56 => "page-next",
        _ => return false,
    };
    send("Action", &(action,).to_variant());
    true
}
/// Nemo 的视图自己把裸空格绑到原生预览接口。改了预览键之后必须把裸空格吞掉，
/// 否则 Nemo 仍会弹它自己的预览，用户看到的配置就没有生效。
fn swallows_plain_space(key: u32, pressed: u32, wanted: &quicklook_core::keys::PreviewKey) -> bool {
    key == 0x20 && pressed == 0 && *wanted != quicklook_core::keys::PreviewKey::default()
}
unsafe extern "C" fn key_press(window: Pointer, event: Pointer, _: Pointer) -> c_int {
    std::panic::catch_unwind(|| {
        let Some(api) = API.get().and_then(Option::as_ref) else {
            return 0;
        };
        let (mut key, mut modifiers) = (0, 0);
        unsafe {
            (api.event_key)(event, &mut key);
            (api.event_state)(event, &mut modifiers);
        }
        if is_repeat(key) {
            return 1;
        }
        let handled = process_key(window, key, modifiers);
        if handled {
            mark_held(key);
        }
        handled as c_int
    })
    .unwrap_or(0)
}
unsafe extern "C" fn key_release(window: Pointer, event: Pointer, _: Pointer) -> c_int {
    let _ = std::panic::catch_unwind(|| {
        if let Some(api) = API.get().and_then(Option::as_ref) {
            let mut key = 0;
            unsafe {
                (api.event_key)(event, &mut key);
            }
            release_key(key);
            if (0xff51..=0xff54).contains(&key) {
                sync_selection(window);
            }
        }
    });
    0
}
/// 窗口级 `key-press-event` 收到事件时，Nemo 的视图通常已经把空格消费掉了。
/// 按键旁路器在事件进入焦点控件之前运行，才能把宿主写死的空格换成配置里的键。
unsafe extern "C" fn key_snooper(widget: Pointer, event: Pointer, _: Pointer) -> c_int {
    std::panic::catch_unwind(|| {
        let Some(api) = API.get().and_then(Option::as_ref) else {
            return 0;
        };
        if widget.is_null() || event.is_null() {
            return 0;
        }
        let kind = unsafe { (api.event_type)(event) };
        let (mut key, mut modifiers) = (0, 0);
        unsafe {
            (api.event_key)(event, &mut key);
            (api.event_state)(event, &mut modifiers);
        }
        if kind == 9 {
            release_key(key);
            return 0;
        }
        if kind != 8 {
            return 0;
        }
        let view = view_for_widget(widget).or_else(|| {
            let window = unsafe { (api.toplevel)(widget) };
            if window.is_null() {
                None
            } else {
                view_for_window(window)
            }
        });
        let Some(view) = view else {
            return 0;
        };
        if is_repeat(key) {
            return 1;
        }
        let wanted = preview_key();
        let pressed = modifiers & (1 | 4 | 8 | 64 | 0x4000000);
        let debug = std::env::var_os("QUICKLOOK_NEMO_DEBUG").is_some();
        if swallows_plain_space(key, pressed, &wanted) {
            if debug {
                eprintln!("Quick Look: 拦截宿主空格（预览键 {}）", wanted.describe());
            }
            return 1;
        }
        let handled = process_key_view(view, key, modifiers);
        if debug {
            eprintln!(
                "Quick Look: 旁路 key={key:#x} state={modifiers:#x} 预览键={} handled={handled}",
                wanted.describe()
            );
        }
        if handled {
            mark_held(key);
        }
        handled as c_int
    })
    .unwrap_or(0)
}
fn install_snooper() {
    if SNOOPER.load(Ordering::Acquire) {
        return;
    }
    let Some(api) = API.get().and_then(Option::as_ref) else {
        return;
    };
    let id = unsafe { (api.snooper_install)(key_snooper, std::ptr::null_mut()) };
    if id == 0 {
        return;
    }
    SNOOPER.store(true, Ordering::Release);
    if std::env::var_os("QUICKLOOK_NEMO_DEBUG").is_some() {
        eprintln!("Quick Look: Nemo 按键旁路器已安装（id={id}）");
    }
}
fn diagnostics() {
    let init = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.diagnostics {
            false
        } else {
            s.diagnostics = true;
            true
        }
    });
    if !init {
        return;
    }
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return;
    };
    let xml = r#"<node><interface name="org.quicklook.NemoAdapter"><method name="Select"><arg type="as" direction="in"/></method><method name="Debug"><arg type="s" direction="out"/></method><method name="Selection"><arg type="as" direction="out"/></method><method name="Key"><arg type="u" direction="in"/><arg type="b" direction="out"/></method><method name="Sync"/></interface></node>"#;
    let xml = if std::env::var_os("QUICKLOOK_ADAPTER_TEST").is_some() {
        xml
    } else {
        r#"<node><interface name="org.quicklook.NemoAdapter"><method name="Status"><arg type="s" direction="out"/></method></interface></node>"#
    };
    let Ok(node) = gio::DBusNodeInfo::for_xml(xml) else {
        return;
    };
    let Some(interface) = node.lookup_interface("org.quicklook.NemoAdapter") else {
        return;
    };
    let _ = connection
        .register_object("/org/quicklook/NemoAdapter", &interface)
        .method_call(|_, _, _, _, method, parameters, invocation| {
            if method=="Status" {invocation.return_value(Some(&("Rust Nemo adapter ready; host executable pinned".to_string(),).to_variant()));return;}
            if method=="Debug" {
                let mut rows=Vec::new();if let Some(api)=API.get().and_then(Option::as_ref){unsafe{let list=(api.windows)();let mut cursor=list;while !cursor.is_null(){let w=(*cursor).data;let mut chain=Vec::new();let mut focus=(api.focus)(w);while !focus.is_null()&&chain.len()<32{chain.push(CStr::from_ptr(go::g_type_name_from_instance(focus.cast())).to_string_lossy().into_owned());focus=(api.parent)(focus);}rows.push(serde_json::json!({"window":CStr::from_ptr(go::g_type_name_from_instance(w.cast())).to_string_lossy(),"focus_chain":chain,"selection":selection(w)}));cursor=(*cursor).next;}gf::g_list_free(list);}}
                invocation.return_value(Some(&(serde_json::Value::Array(rows).to_string(),).to_variant()));return;
            }
            let windows=own_windows();
            let window = windows.iter().copied().find(|&w|view_for_window(w).is_some()&&API.get().and_then(Option::as_ref).map(|api|unsafe{(api.active)(w)!=0}).unwrap_or(false))
                .or_else(||windows.iter().copied().find(|&w|!selection(w).is_empty()))
                .or_else(||windows.into_iter().find(|&w|view_for_window(w).is_some()));
            match (method, window) {
                ("Select",Some(w))=>{
                    if let(Some(api),Some((uris,)))=(API.get().and_then(Option::as_ref),parameters.get::<(Vec<String>,)>()){
                        unsafe{let tree=(api.focus)(w);if is_a(tree,c"GtkTreeView"){let model=(api.tree_model)(tree);let selection=(api.tree_selection)(tree);(api.unselect_all)(selection);let mut iter:TreeIter=std::mem::zeroed();if (api.first)(model,&mut iter)!=0{loop{let mut value:go::GValue=std::mem::zeroed();(api.value)(model,&mut iter,0,&mut value);if go::g_type_is_a(value.g_type,go::G_TYPE_OBJECT)!=0{let file=go::g_value_get_object(&value);if !file.is_null()&&go::g_type_check_instance_is_a(file.cast(),(api.file_type)())!=0{let pointer=(api.file_uri)(file.cast());if !pointer.is_null(){let uri=CStr::from_ptr(pointer).to_string_lossy();if uris.iter().any(|s|s==uri.as_ref()){(api.select_iter)(selection,&mut iter);}gf::g_free(pointer.cast());}}}go::g_value_unset(&mut value);if (api.next)(model,&mut iter)==0{break;}}}}}
                    }invocation.return_value(None);
                }
                ("Selection", Some(w)) => {
                    invocation.return_value(Some(&(selection(w),).to_variant()))
                }
                ("Key", Some(w)) => {
                    let key = parameters.get::<(u32,)>().map(|v| v.0).unwrap_or(0);
                    invocation.return_value(Some(&(process_key(w, key, 0),).to_variant()));
                }
                ("Sync", Some(w)) => {
                    sync_selection(w);
                    invocation.return_value(None);
                }
                _ => invocation.return_dbus_error(
                    "org.quicklook.Error.NotReady",
                    "Nemo file view is not ready",
                ),
            }
        })
        .build();
    let _ = connection.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&("org.quicklook.NemoAdapter", 4u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        1000,
        None::<&gio::Cancellable>,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    use quicklook_core::keys::PreviewKey;
    #[test]
    fn only_plain_space_opens_native_preview() {
        let space = PreviewKey::default();
        assert!(!swallows_plain_space(0x20, 0, &space));
        assert!(!swallows_plain_space(0x20, 4, &space));
    }
    #[test]
    fn remapped_key_swallows_plain_space() {
        for text in ["Ctrl+Space", "Shift+Space", "Return", "F5"] {
            let wanted = PreviewKey::parse(text).unwrap();
            assert!(swallows_plain_space(0x20, 0, &wanted), "{text}");
        }
        let ctrl = PreviewKey::parse("Ctrl+Space").unwrap();
        assert!(!swallows_plain_space(0x20, 4, &ctrl));
        assert!(!swallows_plain_space(0x20, 1, &ctrl));
    }
}
