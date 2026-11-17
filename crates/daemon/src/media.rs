use anyhow::{Result, ensure};
use glib::translate::ToGlibPtr;
use gtk4::{gdk, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[repr(C)]
struct Param {
    kind: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct GlInit {
    get_proc: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    context: *mut c_void,
}
#[repr(C)]
struct Fbo {
    fbo: c_int,
    width: c_int,
    height: c_int,
    format: c_int,
}
#[repr(C)]
struct Event {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}
unsafe extern "C" {
    fn mpv_create() -> *mut c_void;
    fn mpv_initialize(mpv: *mut c_void) -> c_int;
    fn mpv_set_option_string(mpv: *mut c_void, name: *const c_char, value: *const c_char) -> c_int;
    fn mpv_terminate_destroy(mpv: *mut c_void);
    fn mpv_command_async(mpv: *mut c_void, id: u64, args: *const *const c_char) -> c_int;
    fn mpv_get_property(
        mpv: *mut c_void,
        name: *const c_char,
        format: c_int,
        data: *mut c_void,
    ) -> c_int;
    fn mpv_get_property_string(mpv: *mut c_void, name: *const c_char) -> *mut c_char;
    fn mpv_free(pointer: *mut c_void);
    fn mpv_wait_event(mpv: *mut c_void, timeout: f64) -> *const Event;
    fn mpv_error_string(error: c_int) -> *const c_char;
    fn mpv_render_context_create(
        out: *mut *mut c_void,
        mpv: *mut c_void,
        params: *mut Param,
    ) -> c_int;
    fn mpv_render_context_render(render: *mut c_void, params: *mut Param) -> c_int;
    fn mpv_render_context_update(render: *mut c_void) -> u64;
    fn mpv_render_context_set_update_callback(
        render: *mut c_void,
        callback: Option<unsafe extern "C" fn(*mut c_void)>,
        data: *mut c_void,
    );
    fn mpv_render_context_free(render: *mut c_void);
    fn eglGetProcAddress(name: *const c_char) -> *mut c_void;
    fn gdk_wayland_display_get_wl_display(display: *mut gdk::ffi::GdkDisplay) -> *mut c_void;
}
struct Core {
    handle: *mut c_void,
    render: *mut c_void,
    dirty: Box<AtomicBool>,
    error: String,
    background: String,
}
impl Core {
    fn new(background: &str) -> Result<Self> {
        unsafe {
            let handle = mpv_create();
            ensure!(!handle.is_null(), "无法创建 libmpv");
            let core = Self {
                handle,
                render: ptr::null_mut(),
                dirty: Box::new(AtomicBool::new(true)),
                error: String::new(),
                background: background.to_owned(),
            };
            for (name, value) in [
                ("config", "no"),
                ("vo", "libmpv"),
                ("hwdec", "auto-safe"),
                ("terminal", "no"),
                ("input-default-bindings", "no"),
                ("ytdl", "no"),
                ("keep-open", "yes"),
                ("volume", "70"),
            ] {
                let name = CString::new(name)?;
                let value = CString::new(value)?;
                let result = mpv_set_option_string(handle, name.as_ptr(), value.as_ptr());
                ensure!(result >= 0, "libmpv 选项失败");
            }
            let code = mpv_initialize(handle);
            ensure!(code >= 0, "{}", error_string(code));
            Ok(core)
        }
    }
    fn command(&self, args: &[&str]) {
        let strings: Result<Vec<_>, _> = args.iter().map(|s| CString::new(*s)).collect();
        if let Ok(strings) = strings {
            let mut pointers: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
            pointers.push(ptr::null());
            unsafe {
                mpv_command_async(self.handle, 0, pointers.as_ptr());
            }
        }
    }
    fn number(&self, name: &str) -> f64 {
        let mut value = 0.;
        if let Ok(name) = CString::new(name) {
            unsafe {
                mpv_get_property(
                    self.handle,
                    name.as_ptr(),
                    5,
                    (&mut value as *mut f64).cast(),
                );
            }
        }
        value
    }
    fn string(&self, name: &str) -> String {
        let Ok(name) = CString::new(name) else {
            return String::new();
        };
        unsafe {
            let value = mpv_get_property_string(self.handle, name.as_ptr());
            if value.is_null() {
                return String::new();
            }
            let s = CStr::from_ptr(value).to_string_lossy().into_owned();
            mpv_free(value.cast());
            s
        }
    }
    unsafe fn init_render(&mut self, display: &gdk::Display) -> Result<()> {
        unsafe {
            if !self.render.is_null() {
                return Ok(());
            }
            let mut init = GlInit {
                get_proc: resolve_gl,
                context: ptr::null_mut(),
            };
            let mut params = vec![
                Param {
                    kind: 1,
                    data: c"opengl".as_ptr().cast_mut().cast(),
                },
                Param {
                    kind: 2,
                    data: (&mut init as *mut GlInit).cast(),
                },
            ];
            if display.type_().name().contains("Wayland") {
                params.push(Param {
                    kind: 9,
                    data: gdk_wayland_display_get_wl_display(display.to_glib_none().0),
                });
            }
            params.push(Param {
                kind: 0,
                data: ptr::null_mut(),
            });
            let code =
                mpv_render_context_create(&mut self.render, self.handle, params.as_mut_ptr());
            ensure!(code >= 0, "{}", error_string(code));
            mpv_render_context_set_update_callback(
                self.render,
                Some(mark_dirty),
                (&*self.dirty as *const AtomicBool).cast_mut().cast(),
            );
            Ok(())
        }
    }
    unsafe fn free_render(&mut self) {
        if !self.render.is_null() {
            unsafe {
                mpv_render_context_set_update_callback(self.render, None, ptr::null_mut());
                mpv_render_context_free(self.render);
            }
            self.render = ptr::null_mut();
        }
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            self.free_render();
            // The GL context stays on the UI thread; core teardown has no GL
            // resources left and may block while audio/decoder threads finish.
            static REAPER: std::sync::OnceLock<std::sync::mpsc::Sender<usize>> =
                std::sync::OnceLock::new();
            let sender = REAPER.get_or_init(|| {
                let (tx, rx) = std::sync::mpsc::channel::<usize>();
                std::thread::Builder::new()
                    .name("quicklook-mpv-cleanup".into())
                    .spawn(move || {
                        for pointer in rx {
                            mpv_terminate_destroy(pointer as *mut c_void);
                        }
                    })
                    .expect("无法创建媒体清理线程");
                tx
            });
            let _ = sender.send(self.handle as usize);
        }
    }
}
unsafe extern "C" fn resolve_gl(_: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { eglGetProcAddress(name) }
}
unsafe extern "C" fn mark_dirty(data: *mut c_void) {
    if !data.is_null() {
        unsafe { &*data.cast::<AtomicBool>() }.store(true, Ordering::Release);
    }
}
fn error_string(code: i32) -> String {
    unsafe { CStr::from_ptr(mpv_error_string(code)) }
        .to_string_lossy()
        .into_owned()
}

#[derive(Clone)]
pub struct MediaView {
    pub root: gtk4::Box,
    core: Rc<RefCell<Core>>,
}
impl MediaView {
    pub fn new(uri: &str, audio: bool, background: &str) -> Result<Self> {
        let core = Rc::new(RefCell::new(Core::new(background)?));
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let area = gtk4::GLArea::builder()
            .hexpand(true)
            .vexpand(true)
            .auto_render(false)
            .build();
        area.set_required_version(3, 2);
        area.set_allowed_apis(gdk::GLAPI::GL);
        let error_label = gtk4::Label::new(None);
        error_label.set_wrap(true);
        error_label.add_css_class("error");
        error_label.set_visible(false);
        let core2 = core.clone();
        let uri = uri.to_owned();
        let background = background.to_owned();
        let errors = error_label.downgrade();
        area.connect_realize(move |area| {
            area.make_current();
            let result = if let Some(error) = area.error() {
                Err(anyhow::anyhow!(error.to_string()))
            } else {
                unsafe { core2.borrow_mut().init_render(&area.display()) }
            };
            match result {
                Ok(()) => {
                    core2
                        .borrow()
                        .command(&["set", "background-color", &background]);
                    core2.borrow().command(&["loadfile", &uri, "replace"]);
                    if audio {
                        // 音频可视化按窗口拉伸铺满，否则 1024x320 的波形会在上下留下黑边。
                        core2.borrow().command(&["set", "keepaspect", "no"]);
                        let graph = visualizer(1, &background);
                        core2.borrow().command(&["set", "lavfi-complex", &graph]);
                    }
                }
                Err(error) => {
                    core2.borrow_mut().error = error.to_string();
                    if let Some(label) = errors.upgrade() {
                        label.set_text(&format!("媒体渲染初始化失败：{error}"));
                        label.set_visible(true);
                    }
                }
            }
        });
        let core2 = core.clone();
        area.connect_render(move |_, _| {
            unsafe {
                let core = core2.borrow();
                if core.render.is_null() {
                    return glib::Propagation::Proceed;
                }
                let pointer = eglGetProcAddress(c"glGetIntegerv".as_ptr());
                if pointer.is_null() {
                    return glib::Propagation::Proceed;
                }
                let get_integer: unsafe extern "C" fn(u32, *mut i32) = std::mem::transmute(pointer);
                let mut viewport = [0i32; 4];
                let mut framebuffer = 0;
                get_integer(0x0BA2, viewport.as_mut_ptr());
                get_integer(0x8CA6, &mut framebuffer);
                let mut fbo = Fbo {
                    fbo: framebuffer,
                    width: viewport[2],
                    height: viewport[3],
                    format: 0,
                };
                let mut flip = 1i32;
                let mut block = 0i32;
                let mut params = [
                    Param {
                        kind: 3,
                        data: (&mut fbo as *mut Fbo).cast(),
                    },
                    Param {
                        kind: 4,
                        data: (&mut flip as *mut i32).cast(),
                    },
                    Param {
                        kind: 12,
                        data: (&mut block as *mut i32).cast(),
                    },
                    Param {
                        kind: 0,
                        data: ptr::null_mut(),
                    },
                ];
                mpv_render_context_update(core.render);
                mpv_render_context_render(core.render, params.as_mut_ptr());
            }
            glib::Propagation::Stop
        });
        let core2 = core.clone();
        area.connect_unrealize(move |area| {
            area.make_current();
            unsafe {
                core2.borrow_mut().free_render();
            }
        });
        root.append(&area);
        root.append(&error_label);
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        controls.add_css_class("media-controls");
        let play = gtk4::Button::from_icon_name("media-playback-pause-symbolic");
        play.set_tooltip_text(Some("播放 / 暂停（K）"));
        let core2 = core.clone();
        play.connect_clicked(move |_| core2.borrow().command(&["cycle", "pause"]));
        controls.append(&play);
        let time = gtk4::Label::new(Some("00:00"));
        time.set_width_chars(12);
        controls.append(&time);
        let seek = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0., 100., 0.1);
        seek.set_hexpand(true);
        seek.set_draw_value(false);
        let core2 = core.clone();
        let last_seek = Rc::new(Cell::new(None::<std::time::Instant>));
        let last_seek2 = last_seek.clone();
        seek.connect_change_value(move |_, _, value| {
            last_seek2.set(Some(std::time::Instant::now()));
            core2
                .borrow()
                .command(&["seek", &value.to_string(), "absolute+keyframes"]);
            glib::Propagation::Proceed
        });
        let core2 = core.clone();
        let release = gtk4::GestureClick::new();
        let weak = seek.downgrade();
        release.connect_released(move |_, _, _, _| {
            if let Some(seek) = weak.upgrade() {
                core2
                    .borrow()
                    .command(&["seek", &seek.value().to_string(), "absolute+exact"]);
            }
        });
        seek.add_controller(release);
        controls.append(&seek);
        let mute = gtk4::Button::from_icon_name("audio-volume-high-symbolic");
        let core2 = core.clone();
        mute.connect_clicked(move |_| core2.borrow().command(&["cycle", "mute"]));
        controls.append(&mute);
        let volume = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0., 100., 1.);
        volume.set_value(70.);
        volume.set_size_request(80, -1);
        volume.set_draw_value(false);
        let core2 = core.clone();
        volume.connect_change_value(move |_, _, value| {
            core2
                .borrow()
                .command(&["set", "volume", &value.to_string()]);
            glib::Propagation::Proceed
        });
        controls.append(&volume);
        // 用循环按钮而不是 GtkDropDown：下拉菜单会弹出独立的 GtkPopover 表面，
        // 空格键会被菜单吃掉，预览窗口收不到，表现成「空格关不掉」。按钮没有
        // 独立表面，窗口的按键捕获始终有效。
        const SPEEDS: [(&str, &str); 6] = [
            ("0.5", "0.5×"),
            ("0.75", "0.75×"),
            ("1", "1×"),
            ("1.25", "1.25×"),
            ("1.5", "1.5×"),
            ("2", "2×"),
        ];
        let speed_index = Rc::new(Cell::new(2usize));
        let speed = gtk4::Button::with_label(SPEEDS[2].1);
        speed.set_tooltip_text(Some("播放速度"));
        let core2 = core.clone();
        let speed_index2 = speed_index.clone();
        let speed_button = speed.clone();
        speed.connect_clicked(move |_| {
            let next = (speed_index2.get() + 1) % SPEEDS.len();
            speed_index2.set(next);
            speed_button.set_label(SPEEDS[next].1);
            core2.borrow().command(&["set", "speed", SPEEDS[next].0]);
        });
        controls.append(&speed);
        let repeat = gtk4::ToggleButton::builder()
            .icon_name("media-playlist-repeat-symbolic")
            .tooltip_text("循环")
            .build();
        let core2 = core.clone();
        repeat.connect_toggled(move |button| {
            core2.borrow().command(&[
                "set",
                "loop-file",
                if button.is_active() { "inf" } else { "no" },
            ])
        });
        controls.append(&repeat);
        if audio {
            const MODES: [&str; 3] = ["封面 / 音轨", "波形", "频谱"];
            let mode_index = Rc::new(Cell::new(1usize));
            let visualization = gtk4::Button::with_label(MODES[1]);
            visualization.set_tooltip_text(Some("音频可视化"));
            let core2 = core.clone();
            let mode_index2 = mode_index.clone();
            let visualization_button = visualization.clone();
            visualization.connect_clicked(move |_| {
                let next = (mode_index2.get() + 1) % MODES.len();
                mode_index2.set(next);
                visualization_button.set_label(MODES[next]);
                let graph = {
                    let core = core2.borrow();
                    visualizer(next as u32, &core.background)
                };
                core2.borrow().command(&["set", "lavfi-complex", &graph]);
            });
            controls.append(&visualization);
        }
        root.append(&controls);
        let diagnostics = gtk4::Label::new(Some("正在加载媒体…"));
        diagnostics.add_css_class("dim-label");
        diagnostics.set_margin_bottom(8);
        root.append(&diagnostics);
        let area_weak = area.downgrade();
        let play_weak = play.downgrade();
        let seek_weak = seek.downgrade();
        let time_weak = time.downgrade();
        let diag_weak = diagnostics.downgrade();
        let errors = error_label.downgrade();
        let core2 = core.clone();
        let mut ticks = 0;
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(area) = area_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let mut core = core2.borrow_mut();
            if core.dirty.swap(false, Ordering::AcqRel) {
                area.queue_render();
            }
            for _ in 0..32 {
                unsafe {
                    let event = mpv_wait_event(core.handle, 0.);
                    if event.is_null() || (*event).id == 0 {
                        break;
                    }
                    if (*event).id == 7 && !(*event).data.is_null() {
                        let end = &*((*event).data.cast::<EndFile>());
                        if end.reason == 4 {
                            core.error = error_string(end.error);
                        }
                    }
                }
            }
            ticks += 1;
            if ticks % 15 == 0 {
                let duration = core.number("duration");
                let position = core.number("time-pos");
                if let Some(seek) = seek_weak.upgrade() {
                    seek.set_range(0., duration.max(1.));
                    if last_seek
                        .get()
                        .map(|t| t.elapsed() > Duration::from_millis(600))
                        .unwrap_or(true)
                    {
                        seek.set_value(position);
                    }
                }
                if let Some(time) = time_weak.upgrade() {
                    time.set_text(&format!("{} / {}", clock(position), clock(duration)));
                }
                if let Some(play) = play_weak.upgrade() {
                    play.set_icon_name(if core.string("pause") == "yes" {
                        "media-playback-start-symbolic"
                    } else {
                        "media-playback-pause-symbolic"
                    });
                }
                if let Some(diag) = diag_weak.upgrade() {
                    let hw = core.string("hwdec-current");
                    diag.set_text(&format!(
                        "{} · {}",
                        core.string("media-title"),
                        if hw.is_empty() || hw == "no" {
                            "软件解码 / 音频".to_string()
                        } else {
                            format!("硬件解码：{hw}")
                        }
                    ));
                }
                if !core.error.is_empty()
                    && let Some(errors) = errors.upgrade()
                {
                    errors.set_text(&format!(
                        "此媒体无法播放：{}。可使用“属性”查看文件信息。",
                        core.error
                    ));
                    errors.set_visible(true);
                }
            }
            glib::ControlFlow::Continue
        });
        Ok(Self { root, core })
    }
    pub fn command(&self, args: &[&str]) {
        self.core.borrow().command(args);
    }
    pub fn status(&self) -> serde_json::Value {
        let core = self.core.borrow();
        serde_json::json!({"time":core.number("time-pos"),"duration":core.number("duration"),"hwdec":core.string("hwdec-current"),"video_format":core.string("video-format"),"visualizer":core.string("lavfi-complex"),"error":core.error})
    }
}
fn visualizer(mode: u32, background: &str) -> String {
    // showwaves / showspectrum 输出带 alpha；先铺一层配置的背景色再叠波形，
    // 否则 GL 区域会露出棋盘格，和音频播放器的深色面板不一致。
    let color = background.trim_start_matches('#');
    let color = if color.len() == 6 { color } else { "1e1e1e" };
    match mode {
        1 => format!(
            "[aid1]asplit[ao][a];color=c=0x{color}:s=1024x320:rate=30[bg];[a]showwaves=s=1024x320:mode=cline:rate=30:colors=0x60a5fa[wave];[bg][wave]overlay=shortest=1[vo]"
        ),
        2 => format!(
            "[aid1]asplit[ao][a];color=c=0x{color}:s=1024x320:rate=30[bg];[a]showspectrum=s=1024x320:mode=combined:color=rainbow:slide=scroll:scale=log[wave];[bg][wave]overlay=shortest=1[vo]"
        ),
        _ => String::new(),
    }
}
fn clock(value: f64) -> String {
    let seconds = value.max(0.) as u64;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
