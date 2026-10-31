//! X11：把预览窗口保持最上层（EWMH `_NET_WM_STATE_ABOVE`）。
//!
//! GTK4 没有保留 `gtk_window_set_keep_above`，置顶在 Wayland 侧由各合成器的
//! 窗口规则负责（niri 那条 `open-floating` 规则），X11 侧这里按 EWMH 规范
//! 向根窗口发 `_NET_WM_STATE` 客户端消息，由窗口管理器把窗口钉在最上层。
//! 任何失败都静默忽略——置顶是体验增强，不是硬需求，不应影响预览本身。

use glib::translate::ToGlibPtr;
use gtk4::prelude::NativeExt;
use std::os::raw::{c_int, c_long, c_ulong, c_void};

/// 该不该对当前窗口置顶：仅 X11 后端，且不是独立窗口。
/// 独立窗口对标 Wayland 上平铺的 `org.quicklook.Preview.Detached`，
/// 应当能被别的窗口盖住。
pub fn wanted(display_type: &str, detached: bool) -> bool {
    !detached && display_type == "GdkX11Display"
}

const CLIENT_MESSAGE: c_int = 33;
const SUBSTRUCTURE_REDIRECT_MASK: c_long = 1 << 20;
const SUBSTRUCTURE_NOTIFY_MASK: c_long = 1 << 19;
const NET_WM_STATE_ADD: c_long = 1;
const SOURCE_APPLICATION: c_long = 1;

#[repr(C)]
struct XClientMessageEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
    message_type: c_ulong,
    format: c_int,
    data: [c_long; 5],
}

#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(name: *const c_void) -> *mut c_void;
    fn XCloseDisplay(display: *mut c_void) -> c_int;
    fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
    fn XInternAtom(display: *mut c_void, name: *const u8, only_if_exists: c_int) -> c_ulong;
    fn XSendEvent(
        display: *mut c_void,
        window: c_ulong,
        propagate: c_int,
        event_mask: c_long,
        event: *mut XClientMessageEvent,
    ) -> c_int;
    fn XFlush(display: *mut c_void) -> c_int;
}

// GTK4 的 X11 后端合在 `libgtk-4` 里，这个符号随 GTK 一直可用。
unsafe extern "C" {
    fn gdk_x11_surface_get_xid(surface: *const c_void) -> c_ulong;
}

/// 取 GTK 窗口对应的 X11 窗口 id；非 X11 窗口返回 `None`。
pub fn window_xid(window: &gtk4::Window) -> Option<c_ulong> {
    let surface = window.surface()?;
    let pointer: *mut gdk4::ffi::GdkSurface = surface.to_glib_none().0;
    if pointer.is_null() {
        return None;
    }
    let xid = unsafe { gdk_x11_surface_get_xid(pointer.cast()) };
    (xid != 0).then_some(xid)
}

/// 请求窗口管理器把指定 X11 窗口置顶（best effort）。
pub fn set_keep_above(window: c_ulong) {
    unsafe {
        let display = XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return;
        }
        let root = XDefaultRootWindow(display);
        if root != 0 {
            let state = atom(display, b"_NET_WM_STATE\0");
            let above = atom(display, b"_NET_WM_STATE_ABOVE\0");
            if state != 0 && above != 0 {
                request_above(display, root, window, state, above);
            }
        }
        XFlush(display);
        XCloseDisplay(display);
    }
}

fn atom(display: *mut c_void, name: &[u8]) -> c_ulong {
    debug_assert_eq!(name.last(), Some(&0));
    unsafe { XInternAtom(display, name.as_ptr(), 0) }
}

fn request_above(
    display: *mut c_void,
    root: c_ulong,
    window: c_ulong,
    state: c_ulong,
    above: c_ulong,
) {
    unsafe {
        let mut event = XClientMessageEvent {
            type_: CLIENT_MESSAGE,
            serial: 0,
            send_event: 0,
            display,
            window,
            message_type: state,
            format: 32,
            data: [NET_WM_STATE_ADD, above as c_long, 0, SOURCE_APPLICATION, 0],
        };
        XSendEvent(
            display,
            root,
            0,
            SUBSTRUCTURE_REDIRECT_MASK | SUBSTRUCTURE_NOTIFY_MASK,
            &mut event,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::wanted;

    #[test]
    fn keeps_above_only_on_x11_regular_previews() {
        assert!(wanted("GdkX11Display", false));
        assert!(!wanted("GdkX11Display", true));
        assert!(!wanted("GdkWaylandDisplay", false));
    }
}
