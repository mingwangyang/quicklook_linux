mod views;

/// 包内浏览最多展开的层数，防止压缩包套娃把暂存区吃光。
const NESTED_LIMIT: usize = 4;
use crate::{
    backend::{Body, Prepared, Spec},
    canvas::Canvas,
    media::MediaView,
    worker::{Task, Workers},
};
use anyhow::{Context, Result, ensure};
use glib::variant::ToVariant;
use gtk4::{gdk, glib, prelude::*};
use quicklook_core::{
    config::Config,
    files::{FileInfo, Kind, format_size},
    protocol::APP_ID,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    num::NonZeroUsize,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use webkit6::prelude::*;

/// 预览窗口尚未真正可见时，多长时间内的第二次同文件 Toggle 视为「用户想关掉」。
/// 冷启动一次大约 200–600ms，取 1.5s 足够覆盖，又不会误伤正常的再次打开。
const INFLIGHT_TOGGLE: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShowMode {
    Show,
    Toggle,
    Selection,
}

/// 默认浅色：即使桌面整体是深色，也显式关掉 GTK 的深色偏好，
/// 让预览窗口和设置里的主题选择一致。
fn apply_theme(dark: bool) {
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
}
#[derive(Default)]
struct Active {
    canvas: Option<Canvas>,
    text: Option<sourceview5::View>,
    text_bytes: usize,
    media: Option<MediaView>,
    paused: Rc<Cell<bool>>,
    animation_pause: Option<gtk4::ToggleButton>,
    animation_frames: Rc<Cell<u64>>,
    pdf_sidebar: Option<gtk4::ToggleButton>,
}
enum Target {
    Tile {
        generation: u64,
        canvas: glib::WeakRef<Canvas>,
    },
    DirectoryStats {
        generation: u64,
        label: glib::WeakRef<gtk4::Label>,
    },
    Current {
        generation: u64,
        key: String,
    },
    Prefetch {
        key: String,
    },
    Neighbors {
        generation: u64,
    },
    Thumbnail {
        generation: u64,
        picture: glib::WeakRef<gtk4::Picture>,
    },
    PdfPage {
        generation: u64,
        canvas: glib::WeakRef<Canvas>,
    },
    Append {
        generation: u64,
        view: glib::WeakRef<sourceview5::View>,
        button: glib::WeakRef<gtk4::Button>,
    },
    Extract {
        generation: u64,
        /// 进入前的这一层，用于 Esc / Backspace 退回。
        previous: Option<Arc<Prepared>>,
    },
}
struct State {
    selection: Vec<FileInfo>,
    sequence: Vec<FileInfo>,
    index: usize,
    current: Option<FileInfo>,
    prepared: Option<Arc<Prepared>>,
    generation: u64,
    serial: u64,
    pending: HashMap<u64, Target>,
    cancel: Arc<AtomicBool>,
    cache: lru::LruCache<String, Arc<Prepared>>,
    cache_bytes: usize,
    cache_hits: u64,
    active: Active,
    page: i32,
    pages: i32,
    pdf_thumbnails: bool,
    pdf_continuous: bool,
    /// 用户显式选择的连续/单页模式；Some 时不再按配置自动切换。
    pdf_user_mode: Option<bool>,
    /// 用户上次显式选择模式时打开的文件，换文件后自动切换恢复生效。
    pdf_user_file: Option<String>,
    /// PDF 连续滚动模式下所有页共享的缩放倍数。
    pdf_zoom: f64,
    /// 连续模式下已挂载的页 canvas（弱引用），缩放时统一更新。
    pdf_pages: Vec<glib::WeakRef<Canvas>>,
    /// 连续模式的页面宽高比，用于按缩放调整每页高度。
    pdf_aspect: f64,
    /// 连续模式的列表控件（弱引用），Page Up/Down 时用来滚动到上一页/下一页。
    pdf_scroll: Option<glib::WeakRef<gtk4::ScrolledWindow>>,
    pinned: bool,
    /// 分离窗口：独立进程，关闭只结束自己。
    detached: bool,
    ready: bool,
    grid: bool,
    last_error: String,
    request_start: Instant,
    /// 最近一次 show() 的时间，用于识别「窗口还没画出来就又收到一次 Toggle」。
    shown_at: Instant,
    last_elapsed_ms: f64,
    last_action: Instant,
    temporary: Vec<Arc<std::path::PathBuf>>,
    /// 包内浏览栈：每进入一层（压缩包或包内文件），压入进入前的 prepared。
    nested: Vec<Arc<Prepared>>,
    /// Mermaid 流程图的渲染结果回报（`ok:N` / `error:…` / `none` / `unavailable`），空串表示没有图。
    mermaid: String,
}
pub struct Controller {
    inspector: gtk4::Revealer,
    window: gtk4::ApplicationWindow,
    title: gtk4::Label,
    subtitle: gtk4::Label,
    content: gtk4::Box,
    format_bar: gtk4::Box,
    status: gtk4::Label,
    pin: gtk4::ToggleButton,
    config: Config,
    workers: Workers,
    state: RefCell<State>,
    mainloop: glib::MainLoop,
    hidden_exit_ms: Cell<Option<u64>>,
    picker: RefCell<Option<gio::Cancellable>>,
    /// 自己记一份全屏状态：`is_fullscreen()` 要等合成器回传才准，
    /// 拿它当判断依据会出现「按了 F 却没退出全屏」。
    fullscreen: Cell<bool>,
    /// 最近一次主动切换全屏的时间，用来屏蔽合成器回传的滞后事件。
    fullscreen_toggle_at: Cell<Option<Instant>>,
    /// 全屏时浮在内容上方的一行提示。
    fs_hint: gtk4::Label,
    /// 按文件类型设置的内容区背景色（覆盖主题默认）。
    bg_provider: gtk4::CssProvider,
    /// Only presses observed by this window are latched. A new preview must not
    /// wait for a release which may already have happened in the file manager.
    space_held: Cell<bool>,
}
impl Controller {
    pub fn new(app: &gtk4::Application, config: Config, mainloop: glib::MainLoop) -> Rc<Self> {
        apply_theme(config.uses_dark_theme());
        let provider = gtk4::CssProvider::new();
        if config.uses_dark_theme() {
            provider.load_from_string(concat!(
                include_str!("style.css"),
                include_str!("style-dark.css")
            ));
        } else {
            provider.load_from_string(include_str!("style.css"));
        }
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        // 背景色由配置逐次覆盖：单独一张优先级更高的表，换文件时只重载这张。
        let bg_provider = gtk4::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &bg_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("Quick Look")
            .default_width(940)
            .default_height(650)
            .build();
        window.add_css_class("quicklook");
        let header = gtk4::HeaderBar::new();
        let titlebox = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
        let title = gtk4::Label::new(Some("Quick Look"));
        title.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        title.set_single_line_mode(true);
        title.set_max_width_chars(28);
        title.add_css_class("file-title");
        let subtitle = gtk4::Label::new(None);
        subtitle.add_css_class("dim-label");
        titlebox.append(&title);
        header.set_title_widget(Some(&titlebox));
        let prev = icon_button("go-previous-symbolic", "上一个文件（↑）");
        let next = icon_button("go-next-symbolic", "下一个文件（↓）");
        header.pack_start(&prev);
        header.pack_start(&next);
        let pin = gtk4::ToggleButton::builder()
            .label("固定")
            .tooltip_text("固定（P）")
            .build();
        header.pack_end(&pin);
        let full = icon_button("view-fullscreen-symbolic", "全屏（F）");
        header.pack_end(&full);
        window.set_titlebar(Some(&header));
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.set_vexpand(true);
        content.set_hexpand(true);
        content.add_css_class("preview-bg");
        // 点击标题栏/格式栏时 GTK 会把焦点交给按钮；让内容本身可聚焦，
        // 由下面的点击控制器把焦点收回来，空格始终由预览窗口处理。
        content.set_focusable(true);
        let main_area = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        main_area.set_vexpand(true);
        main_area.append(&content);
        let inspector = gtk4::Revealer::builder()
            .transition_type(gtk4::RevealerTransitionType::SlideLeft)
            .build();
        inspector.set_hexpand(false);
        main_area.append(&inspector);
        // 全屏提示：盖在内容区顶部，进入全屏时短暂浮现「退出全屏：F / Esc」。
        let overlay = gtk4::Overlay::new();
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);
        overlay.set_child(Some(&main_area));
        let fs_hint = gtk4::Label::new(Some("全屏中 · 按 F 或 Esc 退出全屏"));
        fs_hint.add_css_class("fullscreen-hint");
        fs_hint.set_halign(gtk4::Align::Center);
        fs_hint.set_valign(gtk4::Align::Start);
        fs_hint.set_margin_top(16);
        fs_hint.set_visible(false);
        overlay.add_overlay(&fs_hint);
        root.append(&overlay);
        let format_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        format_bar.add_css_class("format-bar");
        root.append(&format_bar);
        // 状态提示做成浮在内容底部的胶囊：不占布局高度，几秒后自动淡出，
        // 内容区因此能占到窗口的九成以上。
        let status_pill = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        status_pill.add_css_class("status-pill");
        status_pill.set_halign(gtk4::Align::Start);
        status_pill.set_valign(gtk4::Align::End);
        status_pill.set_margin_start(12);
        status_pill.set_margin_bottom(12);
        let status = gtk4::Label::new(Some("空格关闭 · ↑↓ 浏览 · Enter 打开"));
        status.set_xalign(0.);
        status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        status.set_max_width_chars(70);
        status.add_css_class("dim-label");
        status_pill.append(&status);
        overlay.add_overlay(&status_pill);
        {
            let pill = status_pill.clone();
            status.connect_notify_local(Some("label"), move |label, _| {
                if label.text().is_empty() {
                    pill.set_visible(false);
                    return;
                }
                pill.set_visible(true);
                let expected = label.text().to_string();
                let pill = pill.clone();
                let label = label.clone();
                glib::timeout_add_local(Duration::from_millis(2600), move || {
                    if label.text() == expected {
                        pill.set_visible(false);
                    }
                    glib::ControlFlow::Break
                });
            });
            status_pill.set_visible(false);
        }
        let copy = icon_button("edit-copy-symbolic", "复制文件");
        header.pack_end(&copy);
        let properties = icon_button("document-properties-symbolic", "文件属性（I）");
        header.pack_end(&properties);
        let open_with = gtk4::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("打开方式 / 快捷键")
            .build();
        header.pack_end(&open_with);
        let open = icon_button("document-open-symbolic", "用默认应用打开（Enter）");
        open.add_css_class("suggested-action");
        header.pack_end(&open);
        window.set_child(Some(&root));
        let now = Instant::now();
        let controller = Rc::new(Self {
            inspector,
            window,
            title,
            subtitle,
            content,
            format_bar,
            status,
            pin,
            workers: Workers::new(config.clone()),
            config,
            mainloop,
            hidden_exit_ms: Cell::new(None),
            picker: RefCell::new(None),
            fullscreen: Cell::new(false),
            fullscreen_toggle_at: Cell::new(None),
            fs_hint,
            bg_provider,
            space_held: Cell::new(false),
            state: RefCell::new(State {
                selection: Vec::new(),
                sequence: Vec::new(),
                index: 0,
                current: None,
                prepared: None,
                generation: 0,
                serial: 0,
                pending: HashMap::new(),
                cancel: Arc::new(AtomicBool::new(false)),
                cache: lru::LruCache::new(NonZeroUsize::new(96).unwrap()),
                cache_bytes: 0,
                cache_hits: 0,
                active: Active::default(),
                page: 0,
                pages: 0,
                pdf_thumbnails: false,
                pdf_continuous: false,
                pdf_user_mode: None,
                pdf_user_file: None,
                pdf_zoom: 1.0,
                pdf_pages: Vec::new(),
                pdf_aspect: 1.0,
                pdf_scroll: None,
                pinned: false,
                detached: false,
                ready: false,
                grid: false,
                last_error: String::new(),
                request_start: now,
                shown_at: now,
                last_elapsed_ms: 0.,
                last_action: now,
                temporary: Vec::new(),
                nested: Vec::new(),
                mermaid: String::new(),
            }),
        });
        macro_rules! clicked {($button:expr,$method:ident $(,$arg:expr)*)=>{{let weak=Rc::downgrade(&controller);$button.connect_clicked(move|_|{if let Some(c)=weak.upgrade(){c.$method($($arg),*);}});}};}
        clicked!(prev, navigate, -1);
        clicked!(next, navigate, 1);
        clicked!(full, toggle_fullscreen);
        clicked!(copy, copy_file);
        clicked!(properties, properties);
        clicked!(open, open_default);
        let weak = Rc::downgrade(&controller);
        controller.pin.connect_toggled(move |button| {
            if let Some(c) = weak.upgrade() {
                c.state.borrow_mut().pinned = button.is_active();
            }
        });
        let weak = Rc::downgrade(&controller);
        controller.window.connect_close_request(move |_| {
            if let Some(c) = weak.upgrade() {
                c.close_by_user();
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(&controller);
        controller
            .window
            .connect_notify_local(Some("fullscreen"), move |window, _| {
                if let Some(c) = weak.upgrade() {
                    // 只采信「确实进了全屏」这一个方向：niri 平铺时请求可能不被
                    // 确认为全屏，若把标记改回 false，下一次按 F 又会请求进入，
                    // 看起来就像「F 失灵」。刚刚自己切换过的短时间内也更不采信。
                    if c.fullscreen_recently_toggled() {
                        return;
                    }
                    if window.is_fullscreen() {
                        c.fullscreen.set(true);
                    }
                }
            });
        controller.install_preview_keys(&controller.window);
        // 鼠标点过标题栏、格式栏或状态胶囊后，把焦点还给内容区；否则焦点可能
        // 留在按钮上，用户再按空格时看起来像「空格失效」。这些区域不是
        // content 的子控件，必须把手势分别挂到 header / format_bar / 胶囊上，
        // 挂在 window 上收不到 titlebar 的事件。Capture 阶段保证按钮自己的
        // 点击处理仍然先执行，延迟到 idle 再改焦点，不打断点击。
        let refocus_on_click = |widget: &gtk4::Widget, content: &gtk4::Box| {
            let click = gtk4::GestureClick::new();
            click.set_propagation_phase(gtk4::PropagationPhase::Capture);
            let content = content.clone();
            click.connect_released(move |_, _, _, _| {
                let content = content.clone();
                glib::idle_add_local_once(move || {
                    content.grab_focus();
                });
            });
            widget.add_controller(click);
        };
        refocus_on_click(header.upcast_ref(), &controller.content);
        refocus_on_click(controller.format_bar.upcast_ref(), &controller.content);
        refocus_on_click(status_pill.upcast_ref(), &controller.content);
        // 焦点离开预览窗口时清掉「空格还按着」的记录：抬起事件可能落在别的窗口上，
        // 残留的 true 会让下一次按空格先被当成重复、白按一下。
        let focus = gtk4::EventControllerFocus::new();
        let weak = Rc::downgrade(&controller);
        focus.connect_leave(move |_| {
            if let Some(c) = weak.upgrade() {
                c.space_held.set(false);
            }
        });
        controller.window.add_controller(focus);
        let menu = gtk4::Popover::new();
        let menu_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        menu.set_child(Some(&menu_box));
        open_with.set_popover(Some(&menu));
        // Popover 是独立表面，窗口的 EventControllerKey 收不到里面的按键。
        // 菜单打开时按空格也应该关闭预览，而不是去激活菜单项。
        let menu_key = gtk4::EventControllerKey::new();
        menu_key.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(&controller);
        menu_key.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::space {
                if let Some(c) = weak.upgrade() {
                    c.close_by_user();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        menu.add_controller(menu_key);
        let weak = Rc::downgrade(&controller);
        open_with.connect_notify_local(Some("active"),move|button,_|{if !button.is_active(){return;}
if let Some(c)=weak.upgrade(){clear(&menu_box);if let Some(info)=c.displayed(){for app in gio::AppInfo::all_for_type(&info.mime).into_iter().take(16){let launch=gtk4::Button::with_label(&app.display_name());let file=gio::File::for_uri(&info.uri);launch.connect_clicked(move|_|{if let Err(e)=app.launch(std::slice::from_ref(&file),None::<&gio::AppLaunchContext>){eprintln!("打开方式：{e}");}});menu_box.append(&launch);}}let help=gtk4::Label::new(Some("Space / Esc 关闭 · Ctrl+D 分离窗口\n↑ ↓ 文件导航 · F 全屏 · P 固定\nR 旋转 · 0 适应 · 1 原始像素\nK 暂停 · , . 逐帧 · < > 倍速\nW 换行 · E 编码\nCtrl+C 复制选中文本 / 文件"));help.set_margin_top(12);menu_box.append(&help);let settings=gtk4::Button::with_label("设置…");let weak_settings=Rc::downgrade(&c);settings.connect_clicked(move|_|{if let Some(c)=weak_settings.upgrade(){c.settings();}});menu_box.append(&settings);}});
        let weak = Rc::downgrade(&controller);
        glib::timeout_add_local(Duration::from_millis(8), move || {
            if let Some(c) = weak.upgrade() {
                c.deliver();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        let weak = Rc::downgrade(&controller);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(c) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !c.window.is_visible()
                && c.picker.borrow().is_none()
                && c.hidden_exit_ms
                    .get()
                    .map(|ms| c.state.borrow().last_action.elapsed().as_millis() >= ms as u128)
                    .unwrap_or(false)
            {
                c.mainloop.quit();
            }
            glib::ControlFlow::Continue
        });
        // X11 窗口管理器不会自动把预览窗口钉在最上层（Wayland 那边由
        // 浮动窗口规则负责）。每次窗口映射到屏幕就发一条
        // _NET_WM_STATE_ABOVE，独立窗口除外——它对标 Wayland 上平铺的
        // 独立预览，应当能被别的窗口盖住。
        {
            let weak = Rc::downgrade(&controller);
            controller
                .content
                .connect_notify_local(Some("width"), move |_, _| {
                    if let Some(c) = weak.upgrade() {
                        c.resize_pdf_pages();
                    }
                });
            let weak = Rc::downgrade(&controller);
            controller
                .content
                .connect_notify_local(Some("height"), move |_, _| {
                    if let Some(c) = weak.upgrade() {
                        c.resize_pdf_pages();
                    }
                });
        }
        let weak = Rc::downgrade(&controller);
        controller.window.connect_map(move |_| {
            let Some(c) = weak.upgrade() else {
                return;
            };
            let (backend, detached) = (c.backend(), c.state.borrow().detached);
            if crate::x11::wanted(&backend, detached)
                && let Some(xid) = crate::x11::window_xid(c.window.upcast_ref())
            {
                crate::x11::set_keep_above(xid);
            }
        });
        controller
    }
    pub fn backend(&self) -> String {
        gtk4::prelude::WidgetExt::display(&self.window)
            .type_()
            .name()
            .to_string()
    }
    pub fn set_hidden_exit_ms(&self, value: Option<u64>) {
        self.hidden_exit_ms.set(value);
    }
    /// 标记为分离窗口：关闭只退出本进程，且标题提示可自由平铺。
    pub fn set_detached(&self) {
        self.state.borrow_mut().detached = true;
        self.window.set_title(Some("预览（独立窗口）"));
    }
    /// 在底部状态行显示一条提示。
    pub fn report(&self, text: &str) {
        self.status.set_text(text);
    }
    pub fn handle(
        self: &Rc<Self>,
        method: &str,
        parameters: &glib::Variant,
    ) -> Result<Option<glib::Variant>> {
        if method != "Status" {
            self.state.borrow_mut().last_action = Instant::now();
        }
        match method {
            "Pick" => self.pick(),
            "Preview" | "Show" | "Toggle" | "SetSelection" => {
                let (uris,) = parameters
                    .get::<(Vec<String>,)>()
                    .context("需要 URI 数组")?;
                self.show(
                    uris,
                    match method {
                        "Toggle" => ShowMode::Toggle,
                        "SetSelection" => ShowMode::Selection,
                        _ => ShowMode::Show,
                    },
                )?;
            }
            "Next" => self.navigate(1),
            "Prev" => self.navigate(-1),
            "Close" => self.close_by_user(),
            "Pin" => {
                let (pinned,) = parameters.get::<(bool,)>().context("需要固定状态")?;
                self.pin.set_active(pinned);
            }
            "Status" => return Ok(Some((self.status_json().to_string(),).to_variant())),
            "Action" => {
                let (action,) = parameters.get::<(String,)>().context("需要动作名称")?;
                if action == "continuous" {
                    self.toggle_pdf_continuous(true);
                    return Ok(None);
                }
                if action == "detach" {
                    self.detach();
                    return Ok(None);
                }
                if action == "back" {
                    self.back();
                    return Ok(None);
                }
                let key = match action.as_str() {
                    "fullscreen" => gdk::Key::f,
                    "pin" => gdk::Key::p,
                    "rotate" => gdk::Key::r,
                    "rotate-left" => gdk::Key::R,
                    "fit" => gdk::Key::_0,
                    "actual" => gdk::Key::_1,
                    "pause" => gdk::Key::k,
                    "properties" => gdk::Key::i,
                    "wrap" => gdk::Key::w,
                    "encoding" => gdk::Key::e,
                    "thumbnails" => gdk::Key::t,
                    "page-prev" => gdk::Key::Page_Up,
                    "page-next" => gdk::Key::Page_Down,
                    _ => anyhow::bail!("未知动作"),
                };
                self.key(
                    key,
                    if action == "rotate-left" {
                        gdk::ModifierType::SHIFT_MASK
                    } else {
                        gdk::ModifierType::empty()
                    },
                );
            }
            "Screenshot" => {
                let (path,) = parameters.get::<(String,)>().context("需要 PNG 路径")?;
                ensure!(self.window.is_visible(), "当前没有打开预览窗口");
                let paintable = gtk4::WidgetPaintable::new(Some(&self.window));
                let snapshot = gtk4::Snapshot::new();
                paintable.snapshot(
                    &snapshot,
                    self.window.width() as f64,
                    self.window.height() as f64,
                );
                let node = snapshot.to_node().context("窗口尚未完成绘制")?;
                let renderer = self.window.renderer().context("渲染器不可用")?;
                renderer.render_texture(&node, None).save_to_png(path)?;
            }
            "Page" => {
                let (page,) = parameters.get::<(i32,)>().context("需要页码")?;
                let current = self
                    .state
                    .borrow()
                    .current
                    .clone()
                    .context("没有当前文件")?;
                ensure!(
                    self.state.borrow().pages > 0 && page >= 1,
                    "仅 PDF 支持从 1 开始的页码"
                );
                self.load(
                    current,
                    Spec {
                        page: page - 1,
                        ..Default::default()
                    },
                );
            }
            "Extract" => {
                let (index,) = parameters.get::<(u32,)>().context("需要条目序号")?;
                let current = self.displayed().context("没有当前压缩包")?;
                ensure!(current.kind == Kind::Archive, "当前文件不是压缩包");
                let previous = self.state.borrow().prepared.clone();
                self.begin();
                let generation = self.state.borrow().generation;
                self.submit(
                    Task::Extract(current, index as usize),
                    Target::Extract {
                        generation,
                        previous,
                    },
                    false,
                );
            }
            "Back" => {
                if !self.back() {
                    anyhow::bail!("已经在最外层");
                }
            }
            "Quit" => self.mainloop.quit(),
            _ => anyhow::bail!("未知方法"),
        }
        Ok(None)
    }
    pub fn show(self: &Rc<Self>, uris: Vec<String>, mode: ShowMode) -> Result<()> {
        ensure!(uris.len() <= 4096, "最多选择 4096 个文件");
        if mode == ShowMode::Selection
            && (!self.window.is_visible()
                || self.state.borrow().pinned
                || self.picker.borrow().is_some())
        {
            return Ok(());
        }
        if uris.is_empty() {
            if mode == ShowMode::Selection {
                self.close();
                return Ok(());
            }
            anyhow::bail!("没有选择文件");
        }
        let infos = uris
            .iter()
            .map(FileInfo::from_arg)
            .collect::<Result<Vec<_>>>()?;
        let picker = self.picker.borrow_mut().take();
        if let Some(picker) = picker {
            picker.cancel();
        }
        let same = self
            .state
            .borrow()
            .selection
            .iter()
            .map(|f| &f.uri)
            .eq(infos.iter().map(|f| &f.uri));
        // 窗口还没画出来时又收到一次同一个文件的 Toggle：多半是用户很快按了第二下想关掉。
        // 若不处理，这次 Toggle 会把同一个文件再 show 一遍，窗口照样留在屏幕上，
        // 用户就会觉得「空格有时候关不掉，要再按一次」。
        let inflight = {
            let s = self.state.borrow();
            // `shown_at` 在 Controller 创建时就初始化；刚启动的 viewer 在前 1.5 秒内
            // 收到第一次 Toggle 时窗口当然还不可见，但这不是「还没画完又按了一次」。
            // 只有已经接过一次 Show/Toggle（selection 非空）才可能是连按。
            !s.selection.is_empty()
                && !self.window.is_visible()
                && s.shown_at.elapsed() < INFLIGHT_TOGGLE
        };
        // Toggle 的语义是「按空格切换预览」：窗口已经可见时，无论宿主此刻报的是
        // 哪一个文件，都应当关闭。此前要求 URI 完全相同才关闭，用户在文件管理器里
        // 改选过文件、或宿主没有同步 SetSelection 时，就会变成「又打开一个新文件」，
        // 看起来就是空格关不掉。
        if mode == ShowMode::Toggle && (self.window.is_visible() || inflight) {
            self.close();
            return Ok(());
        }
        if mode == ShowMode::Selection && same {
            return Ok(());
        }
        {
            let mut s = self.state.borrow_mut();
            s.selection = infos.clone();
            s.sequence = infos.clone();
            s.index = 0;
            s.page = 0;
            s.last_action = Instant::now();
        }
        self.size_window(None);
        self.state.borrow_mut().shown_at = Instant::now();
        self.space_held.set(false);
        if infos.len() > 1 {
            self.present_preview();
            self.show_grid(infos);
        } else {
            // 单文件先算尺寸再显示：GTK4 的 set_default_size 对已映射窗口无效，
            // 图片/PDF 需要按页面比例开窗，只能等首帧渲染完再 present。
            let info = infos[0].clone();
            self.load(info.clone(), Spec::default());
            let generation = self.state.borrow().generation;
            self.submit(
                Task::Prepare(
                    FileInfo::from_path(info.path.parent().context("无父目录")?)?,
                    Spec::default(),
                ),
                Target::Neighbors { generation },
                true,
            );
        }
        Ok(())
    }
    /// 显示预览窗，并显式请求键盘焦点。
    ///
    /// `present()` 在部分 Wayland 合成器上只负责映射窗口，不会把键盘焦点从文件
    /// 管理器转过来；用户必须先点一下预览窗，下一次空格才会落到 viewer 的按键处理。
    /// 这里同时调用 GTK 的焦点请求和 GDK toplevel 的 focus，并在一小段延迟后重试，
    /// 覆盖窗口刚映射、niri 还没应用完窗口规则的时间窗。
    /// 在预览窗口和 WebView 上都安装同一套按键控制器。
    ///
    /// WebKit 会把键盘焦点吞到自己的子窗口里，只挂在窗口上的 Capture
    /// 控制器在 Markdown 渲染时收不到 key release，空格就会「按下去有记录、
    /// 抬不起来」，表现为关不掉。给 WebView 单独挂一份，保证 press/release
    /// 都走同一个 `key()`/`space_held` 状态机。
    fn install_preview_keys(self: &Rc<Self>, widget: &impl IsA<gtk4::Widget>) {
        let key = gtk4::EventControllerKey::new();
        key.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        key.connect_key_pressed(move |_, key, _, modifiers| {
            weak.upgrade()
                .map(|c| c.key(key, modifiers))
                .unwrap_or(glib::Propagation::Proceed)
        });
        let weak = Rc::downgrade(self);
        key.connect_key_released(move |_, key, _, _| {
            if key == gdk::Key::space
                && let Some(c) = weak.upgrade()
                && c.space_held.replace(false)
                && c.window.is_visible()
            {
                c.close_by_user();
            }
        });
        widget.add_controller(key);
    }

    fn present_preview(&self) {
        self.window.present();
        self.focus_preview();
        // 文件夹预览等内容会把键盘焦点交给内部的 ListView；第二次显示时
        // ListView 仍保留焦点，空格会被列表控件吃掉。每次显示后把焦点
        // 拉回内容容器，保证窗口级按键控制器能收到 press/release。
        self.content.grab_focus();
        let window = self.window.downgrade();
        let content = self.content.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(120), move || {
            if let Some(window) = window.upgrade() {
                window.set_can_focus(true);
                window.grab_focus();
                if let Some(toplevel) = window
                    .surface()
                    .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
                {
                    toplevel.focus(gdk::CURRENT_TIME);
                }
            }
            if let Some(content) = content.upgrade() {
                content.grab_focus();
            }
        });
    }

    fn focus_preview(&self) {
        self.window.set_can_focus(true);
        self.window.grab_focus();
        if let Some(toplevel) = self
            .window
            .surface()
            .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        {
            toplevel.focus(gdk::CURRENT_TIME);
        }
    }

    fn size_window(&self, aspect: Option<f64>) {
        let monitors = gtk4::prelude::WidgetExt::display(&self.window).monitors();
        let monitor = self
            .window
            .surface()
            .and_then(|s| gtk4::prelude::WidgetExt::display(&self.window).monitor_at_surface(&s))
            .or_else(|| monitors.item(0).and_downcast::<gdk::Monitor>());
        if let Some(monitor) = monitor {
            let r = monitor.geometry();
            let (w, h) = preview_size((r.width(), r.height()), aspect, self.config.window_size());
            self.window.set_default_size(w, h);
        }
    }
    fn begin(&self) {
        let mut s = self.state.borrow_mut();
        s.cancel.store(true, Ordering::Relaxed);
        s.cancel = Arc::new(AtomicBool::new(false));
        s.generation += 1;
        s.ready = false;
        s.mermaid.clear();
        s.request_start = Instant::now();
        s.last_error.clear();
        s.prepared = None;
        s.active = Active::default();
        s.pending.clear();
        drop(s);
        self.workers.cancel_prefetch();
        clear(&self.content);
        clear(&self.format_bar);
    }
    fn load(self: &Rc<Self>, info: FileInfo, spec: Spec) {
        self.begin();
        self.state.borrow_mut().nested.clear();
        let generation = self.state.borrow().generation;
        let key = spec.key(&info);
        let file_key = info.key();
        {
            let mut s = self.state.borrow_mut();
            let same_file =
                s.current.as_ref().map(|c| c.key()).as_deref() == Some(file_key.as_str());
            s.current = Some(info.clone());
            s.page = spec.page;
            s.pages = 0;
            s.pdf_continuous = false;
            s.pdf_scroll = None;
            if !same_file {
                s.pdf_user_mode = None;
                s.pdf_user_file = None;
            }
            s.grid = false;
        }
        self.title.set_text(&info.name);
        self.subtitle.set_text(&info.title());
        self.title.set_tooltip_text(Some(&info.title()));
        self.window
            .set_title(Some(&format!("{} — Quick Look", info.name)));
        self.status.set_text("正在准备预览…");
        let cached = {
            let mut s = self.state.borrow_mut();
            let value = s.cache.get(&key).cloned();
            if value.is_some() {
                s.cache_hits += 1;
            }
            value
        };
        if let Some(prepared) = cached {
            self.render(prepared);
        } else {
            let spinner = gtk4::Spinner::new();
            spinner.set_halign(gtk4::Align::Center);
            spinner.set_valign(gtk4::Align::Center);
            spinner.set_vexpand(true);
            spinner.start();
            self.content.append(&spinner);
            self.submit(
                Task::Prepare(info, spec),
                Target::Current { generation, key },
                false,
            );
        }
    }
    fn submit(&self, task: Task, target: Target, low: bool) {
        let mut s = self.state.borrow_mut();
        s.serial += 1;
        let id = s.serial;
        let cancel = s.cancel.clone();
        if self.workers.submit(id, task, cancel, low) {
            s.pending.insert(id, target);
        }
    }
    fn cache(&self, key: String, content: Arc<Prepared>) {
        let weight = content.weight();
        let max = self.config.cache_mb * 1024 * 1024;
        if weight > max {
            return;
        }
        let mut s = self.state.borrow_mut();
        if let Some(old) = s.cache.pop(&key) {
            s.cache_bytes = s.cache_bytes.saturating_sub(old.weight());
        }
        while s.cache_bytes + weight > max || s.cache.len() >= 95 {
            if let Some((_, old)) = s.cache.pop_lru() {
                s.cache_bytes = s.cache_bytes.saturating_sub(old.weight());
            } else {
                break;
            }
        }
        s.cache_bytes += weight;
        s.cache.put(key, content);
    }
    fn deliver(self: &Rc<Self>) {
        for _ in 0..24 {
            let Ok(delivery) = self.workers.receiver.try_recv() else {
                break;
            };
            let target = self.state.borrow_mut().pending.remove(&delivery.id);
            let Some(target) = target else {
                continue;
            };
            let generation = self.state.borrow().generation;
            match target {
                Target::Tile {
                    generation: g,
                    canvas,
                } if g == generation => {
                    if let (
                        Some(canvas),
                        Ok(Prepared {
                            body: Body::Tile { pixels, rect },
                            ..
                        }),
                    ) = (canvas.upgrade(), delivery.result)
                    {
                        canvas.set_detail(&pixels, rect);
                    }
                }
                Target::DirectoryStats {
                    generation: g,
                    label,
                } if g == generation => {
                    if let (Some(label), Ok(prepared)) = (label.upgrade(), delivery.result)
                        && let Body::Summary(text) = prepared.body
                    {
                        label.set_text(&text);
                    }
                }
                Target::Current { generation: g, key } if g == generation => {
                    match delivery.result {
                        Ok(content) => {
                            let content = Arc::new(content);
                            self.cache(key, content.clone());
                            self.render(content);
                            self.prefetch();
                        }
                        Err(error) => self.error(&error),
                    }
                }
                Target::Prefetch { key } => {
                    if let Ok(content) = delivery.result {
                        self.cache(key, Arc::new(content));
                    }
                }
                Target::Neighbors { generation: g } if g == generation => {
                    if let Ok(Prepared {
                        body: Body::Directory(mut data),
                        ..
                    }) = delivery.result
                    {
                        data.entries.sort_by(|a, b| {
                            natord::compare(&a.name.to_lowercase(), &b.name.to_lowercase())
                        });
                        let current = self.state.borrow().current.clone();
                        if let Some(info) = current {
                            if !info.name.starts_with('.') {
                                data.entries.retain(|f| !f.name.starts_with('.'));
                            }
                            let index = data
                                .entries
                                .iter()
                                .position(|f| f.uri == info.uri)
                                .unwrap_or(0);
                            let mut s = self.state.borrow_mut();
                            s.sequence = data.entries;
                            s.index = index;
                            drop(s);
                            self.prefetch();
                        }
                    }
                }
                Target::Thumbnail {
                    generation: g,
                    picture,
                } if g == generation => {
                    if let (Some(picture), Ok(content)) = (picture.upgrade(), delivery.result) {
                        let pixels = match content.body {
                            Body::Image(i) => i.frames.first().map(|f| f.0.clone()),
                            Body::Pdf(p) => Some(p.pixels),
                            _ => None,
                        };
                        if let Some(pixels) = pixels {
                            picture.set_paintable(Some(&texture(&pixels)));
                        }
                    }
                }
                Target::PdfPage {
                    generation: g,
                    canvas,
                } if g == generation => {
                    if let (Some(canvas), Ok(content)) = (canvas.upgrade(), delivery.result)
                        && let Body::Pdf(pdf) = content.body
                    {
                        canvas.set_pdf(&pdf);
                    }
                }
                Target::Append {
                    generation: g,
                    view,
                    button,
                } if g == generation => {
                    if let (Some(view), Some(button), Ok(content)) =
                        (view.upgrade(), button.upgrade(), delivery.result)
                        && let Body::Text(chunk) = content.body
                    {
                        let buffer = view.buffer();
                        if self.state.borrow().active.text_bytes + chunk.text.len()
                            <= self.config.max_text_buffer_bytes
                        {
                            self.state.borrow_mut().active.text_bytes += chunk.text.len();
                            buffer.insert(&mut buffer.end_iter(), &chunk.text);
                            button.set_sensitive(true);
                            button.set_label("继续加载");
                            button.set_visible(chunk.next_offset < chunk.total);
                            self.state.borrow_mut().prepared = Some(Arc::new(Prepared {
                                info: content.info,
                                body: Body::Text(chunk),
                                note: content.note,
                                temporary: content.temporary,
                            }));
                        } else {
                            button.set_label("已达文本缓冲区上限，请使用默认编辑器");
                        }
                    }
                }
                Target::Extract {
                    generation: g,
                    previous,
                } if g == generation => match delivery.result {
                    Ok(content) => {
                        let depth = self.state.borrow().nested.len();
                        if content.info.kind == Kind::Archive && depth >= NESTED_LIMIT {
                            self.error(&format!(
                                "嵌套压缩包最多展开 {NESTED_LIMIT} 层，请逐层返回后再继续"
                            ));
                        } else {
                            if let Some(dir) = content.temporary.clone() {
                                self.state.borrow_mut().temporary.push(dir);
                            }
                            // 记住进入前的一层，Esc / Backspace 可以退回。
                            if let Some(previous) = previous {
                                self.state.borrow_mut().nested.push(previous);
                            }
                            self.render(Arc::new(content));
                        }
                    }
                    Err(error) => self.error(&error),
                },
                _ => {}
            }
        }
    }
    fn prefetch(&self) {
        let items = {
            let s = self.state.borrow();
            let start = s.index.saturating_sub(3);
            let end = (s.index + 4).min(s.sequence.len());
            s.sequence
                .get(start..end)
                .unwrap_or_default()
                .iter()
                .filter(|i| Some(&i.uri) != s.current.as_ref().map(|c| &c.uri))
                .filter(|i| {
                    matches!(
                        i.kind,
                        Kind::Image | Kind::Text | Kind::Pdf | Kind::Markdown
                    )
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        for info in items {
            let spec = Spec::default();
            let key = spec.key(&info);
            if !self.state.borrow().cache.contains(&key)
                && !self
                    .state
                    .borrow()
                    .pending
                    .values()
                    .any(|t| matches!(t, Target::Prefetch { key: k } if k == &key))
            {
                self.submit(Task::Prepare(info, spec), Target::Prefetch { key }, true);
            }
        }
    }
    fn pick(self: &Rc<Self>) {
        if self.picker.borrow().is_some() {
            return;
        }
        let dialog = gtk4::FileDialog::builder()
            .title("选择要预览的文件")
            .accept_label("预览")
            .modal(true)
            .build();
        let cancel = gio::Cancellable::new();
        self.picker.replace(Some(cancel.clone()));
        let request = cancel.clone();
        let weak = Rc::downgrade(self);
        dialog.open_multiple(Some(&self.window), Some(&cancel), move |result| {
            let Some(c) = weak.upgrade() else {
                return;
            };
            if c.picker.borrow().as_ref() != Some(&request) {
                return;
            }
            c.picker.borrow_mut().take();
            c.state.borrow_mut().last_action = Instant::now();
            if let Ok(files) = result {
                let uris = files
                    .iter::<gio::File>()
                    .filter_map(Result::ok)
                    .map(|f| f.uri().to_string())
                    .collect::<Vec<_>>();
                if !uris.is_empty()
                    && let Err(error) = c.show(uris, ShowMode::Show)
                {
                    c.error(&format!("{error:#}"));
                    c.window.set_visible(true);
                }
            }
        });
    }
    fn close_by_user(&self) {
        self.close();
    }
    pub fn close(&self) {
        let picker = self.picker.borrow_mut().take();
        if let Some(picker) = picker {
            picker.cancel();
        }
        self.window.set_visible(false);
        self.begin();
        let mut s = self.state.borrow_mut();
        s.current = None;
        s.selection.clear();
        s.sequence.clear();
        s.nested.clear();
        s.cache.clear();
        s.cache_bytes = 0;
        s.pinned = false;
        s.last_action = Instant::now();
        drop(s);
        self.pin.set_active(false);
        if self.hidden_exit_ms.get() == Some(0) {
            self.mainloop.quit();
        }
    }
    /// 当前预览复制到一个独立进程窗口，本窗口随即关闭，可继续用空格开新窗口。
    fn detach(self: &Rc<Self>) {
        let (uris, already) = {
            let s = self.state.borrow();
            let uris: Vec<String> = if s.grid {
                s.selection.iter().map(|info| info.uri.clone()).collect()
            } else {
                s.current.iter().map(|info| info.uri.clone()).collect()
            };
            (uris, s.detached)
        };
        if already {
            self.status.set_text("当前已是独立窗口");
            return;
        }
        if uris.is_empty() {
            self.status.set_text("没有可分离的预览");
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            let exe = std::env::current_exe()?
                .parent()
                .context("无法定位预览程序")?
                .join("quicklook-viewer");
            let mut command = std::process::Command::new(exe);
            command.arg("--detached");
            for uri in &uris {
                command.arg(format!("--initial-uri={uri}"));
            }
            command.spawn().context("无法启动独立窗口")?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.status
                    .set_text("已分离为独立窗口，关闭它不会影响后续预览");
                self.close_by_user();
            }
            Err(error) => self.status.set_text(&format!("分离失败：{error:#}")),
        }
    }
    /// 当前真正显示的文件：包内浏览时是解压出来的那一份，而不是外层压缩包。
    fn displayed(&self) -> Option<FileInfo> {
        let s = self.state.borrow();
        s.prepared
            .as_ref()
            .map(|p| p.info.clone())
            .or_else(|| s.current.clone())
    }
    /// 回到上一层（包内文件 → 压缩包列表 → 退出）。返回是否处理了这次返回。
    fn back(self: &Rc<Self>) -> bool {
        let previous = self.state.borrow_mut().nested.pop();
        let Some(previous) = previous else {
            return false;
        };
        self.render(previous);
        true
    }
    /// 依据包内浏览栈刷新标题与副标题，形成 `a.zip › b.tar › file.txt` 式面包屑。
    fn refresh_titles(&self) {
        let (outer, name, subtitle, detached) = {
            let s = self.state.borrow();
            let outer: Vec<String> = s.nested.iter().map(|p| p.info.name.clone()).collect();
            let info = s
                .prepared
                .as_ref()
                .map(|p| p.info.clone())
                .or_else(|| s.current.clone());
            match info {
                Some(info) => (outer, info.name.clone(), info.title(), s.detached),
                None => return,
            }
        };
        let mut crumb = outer.join(" › ");
        if !crumb.is_empty() {
            crumb.push_str(" › ");
        }
        crumb.push_str(&name);
        if detached {
            crumb.push_str("（独立窗口）");
        }
        self.title.set_text(&crumb);
        self.subtitle.set_text(&subtitle);
        self.title.set_tooltip_text(Some(&subtitle));
        self.window
            .set_title(Some(&format!("{crumb} — Quick Look")));
    }
    fn navigate(self: &Rc<Self>, direction: i32) {
        let info = {
            let mut s = self.state.borrow_mut();
            // 包内浏览时 ↑↓ 不跳出压缩包，避免切到外层同级文件。
            if s.pinned || !s.nested.is_empty() || s.sequence.is_empty() {
                return;
            }
            let index =
                (s.index as i64 + direction as i64).clamp(0, s.sequence.len() as i64 - 1) as usize;
            s.index = index;
            s.sequence[index].clone()
        };
        self.load(info, Spec::default());
    }
    /// 刚刚自己切换过全屏（短时间内不采信合成器的滞后回报）。
    fn fullscreen_recently_toggled(&self) -> bool {
        self.fullscreen_toggle_at
            .get()
            .map(|at: Instant| at.elapsed() < Duration::from_millis(600))
            .unwrap_or(false)
    }
    /// 进入/退出全屏时给用户看得见的提示，避免「点了全屏不知道怎样还原」。
    fn set_fullscreen_hint(&self, on: bool) {
        self.fs_hint.set_visible(on);
        if on {
            self.status.set_text("全屏中 · 按 F 或 Esc 退出全屏");
            let weak = self.fs_hint.downgrade();
            glib::timeout_add_seconds_local_once(5, move || {
                if let Some(label) = weak.upgrade() {
                    label.set_visible(false);
                }
            });
        } else {
            self.status.set_text("空格关闭 · ↑↓ 浏览 · Enter 打开");
        }
    }
    fn toggle_fullscreen(&self) {
        self.fullscreen_toggle_at.set(Some(Instant::now()));
        // 自己也可能是被合成器（比如 niri 的 Mod+F）全屏的，那种情况下
        // `is_fullscreen()` 为真但我们的标记还是 false，按一次应该退出而不是再请求一次。
        let full = self.fullscreen.get() || self.window.is_fullscreen();
        if full {
            self.window.unfullscreen();
            self.fullscreen.set(false);
            self.set_fullscreen_hint(false);
        } else {
            self.window.fullscreen();
            self.fullscreen.set(true);
            self.set_fullscreen_hint(true);
        }
    }
    /// 退出全屏；本来就不在全屏时返回 false。
    fn leave_fullscreen(&self) -> bool {
        if !self.fullscreen.get() && !self.window.is_fullscreen() {
            return false;
        }
        self.fullscreen_toggle_at.set(Some(Instant::now()));
        self.window.unfullscreen();
        self.fullscreen.set(false);
        self.set_fullscreen_hint(false);
        true
    }
    /// 连续滚动模式：所有页一起按倍数缩放，并同步每页高度让滚动范围正确。
    /// 每页高度取「按宽度排满」和「按高度排满」中较小的那个，再乘缩放。
    fn pdf_page_height(&self, aspect: f64, zoom: f64) -> i32 {
        // 页面按「宽度和高度都不溢出」缩放：竖排 PDF 在宽窗口里由高度决定，
        // 横排页面由宽度决定；不能只看宽度，否则竖页会缩成窄条。
        let width = self.content.width().max(320) as f64;
        let height = self.content.height().max(200) as f64;
        let fit = (width * aspect.max(0.05)).min(height);
        (fit * zoom).clamp(120., 20_000.) as i32
    }
    fn resize_pdf_pages(&self) {
        let (continuous, aspect, zoom, pages) = {
            let s = self.state.borrow();
            (
                s.pdf_continuous,
                s.pdf_aspect,
                s.pdf_zoom,
                s.pdf_pages.clone(),
            )
        };
        if !continuous {
            return;
        }
        let height = self.pdf_page_height(aspect, zoom);
        for weak in &pages {
            if let Some(canvas) = weak.upgrade() {
                canvas.set_size_request(-1, height);
            }
        }
    }
    fn zoom_all_pages(&self, factor: f64) {
        let mut s = self.state.borrow_mut();
        let z = (s.pdf_zoom * factor).clamp(0.25, 8.0);
        s.pdf_zoom = z;
        let aspect = s.pdf_aspect;
        let pages = s.pdf_pages.clone();
        drop(s);
        let height = self.pdf_page_height(aspect, z);
        for weak in &pages {
            if let Some(canvas) = weak.upgrade() {
                canvas.set_zoom(z);
                canvas.set_size_request(-1, height);
            }
        }
    }
    fn reset_pdf_zoom(&self) {
        let current = self.state.borrow().pdf_zoom;
        if (current - 1.0).abs() > f64::EPSILON {
            self.zoom_all_pages(1.0 / current);
        }
    }
    fn copy_file(&self) {
        if let Some(info) = self.displayed() {
            use std::process::Command;
            let result = (|| -> anyhow::Result<()> {
                let helper = std::env::current_exe()?
                    .parent()
                    .context("无法定位剪贴板程序")?
                    .join("quicklook-clipboard");
                anyhow::ensure!(
                    Command::new(helper).arg(&info.uri).status()?.success(),
                    "剪贴板写入失败"
                );
                Ok(())
            })();
            self.status.set_text(if result.is_ok() {
                "已复制文件，关闭预览后仍可粘贴"
            } else {
                "文件复制失败，请检查 Wayland 剪贴板"
            });
        }
    }
    /// 打开设置窗口；配置改完由设置进程自己写回并应用集成。
    fn settings(&self) {
        let result = (|| -> anyhow::Result<()> {
            let exe = std::env::current_exe()?
                .parent()
                .context("无法定位程序目录")?
                .join("quicklook-settings");
            anyhow::ensure!(exe.is_file(), "找不到 {}", exe.display());
            std::process::Command::new(exe).spawn()?;
            Ok(())
        })();
        if let Err(error) = result {
            self.status.set_text(&format!("无法打开设置：{error:#}"));
        }
    }
    fn open_default(&self) {
        let info = self.displayed();
        if let Some(info) = info {
            match gio::AppInfo::launch_default_for_uri(&info.uri, None::<&gio::AppLaunchContext>) {
                Ok(()) => {
                    let state = self.state.borrow();
                    if !state.pinned && !state.detached {
                        drop(state);
                        self.close_by_user();
                    }
                }
                Err(e) => self.status.set_text(&format!("打开失败：{e}")),
            }
        }
    }
    fn key(self: &Rc<Self>, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        if key == gdk::Key::Escape {
            // 全屏时先退出全屏，再按一次才回上一层/关闭，和其他预览器的习惯一致。
            if !self.leave_fullscreen() && !self.back() {
                self.close_by_user();
            }
            return glib::Propagation::Stop;
        }
        // Page Up / Page Down 始终翻页，即使焦点停在页码输入框里也一样——
        // 用户按这两个键就是想把整页翻过去，而不是往输入框里打字。
        if matches!(key, gdk::Key::Page_Down | gdk::Key::KP_Page_Down) {
            self.pdf_page_step(1);
            return glib::Propagation::Stop;
        }
        if matches!(key, gdk::Key::Page_Up | gdk::Key::KP_Page_Up) {
            self.pdf_page_step(-1);
            return glib::Propagation::Stop;
        }
        // 空格永远是关闭键，即使焦点落在页码输入框、格式栏按钮或文本视图里。
        // 此前 `editing` 分支会把页码 SpinButton 里的空格直接放行，用户点过
        // 页码之后再按空格就关不掉预览，看起来像「有时候空格失灵」。
        if space_closes(key, modifiers) {
            self.space_held.set(true);
            return glib::Propagation::Stop;
        }
        let editing = gtk4::prelude::RootExt::focus(&self.window)
            .map(|w| w.is::<gtk4::Editable>() || w.is::<gtk4::SpinButton>())
            .unwrap_or(false);
        if editing {
            return glib::Propagation::Proceed;
        }
        if ctrl && key == gdk::Key::c {
            if let Some(focus) = gtk4::prelude::RootExt::focus(&self.window) {
                if let Some(view) = focus.downcast_ref::<gtk4::TextView>()
                    && view.buffer().has_selection()
                {
                    return glib::Propagation::Proceed;
                }
                if let Some(web) = focus.downcast_ref::<webkit6::WebView>()
                    && web
                        .editor_state()
                        .map(|e| e.is_copy_available())
                        .unwrap_or(false)
                {
                    return glib::Propagation::Proceed;
                }
                if let Some(canvas) = focus.downcast_ref::<Canvas>() {
                    let text = canvas.selected_text();
                    if !text.is_empty() {
                        self.window.clipboard().set_text(&text);
                        return glib::Propagation::Stop;
                    }
                }
            }
            self.copy_file();
            return glib::Propagation::Stop;
        }
        if ctrl && key == gdk::Key::d {
            self.detach();
            return glib::Propagation::Stop;
        }
        if ctrl {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::BackSpace => {
                if !self.back() {
                    self.status.set_text("已经在最外层");
                }
            }
            gdk::Key::Up => self.navigate(-1),
            gdk::Key::Down => self.navigate(1),
            gdk::Key::Return | gdk::Key::KP_Enter => self.open_default(),
            gdk::Key::f | gdk::Key::F => self.toggle_fullscreen(),
            gdk::Key::p | gdk::Key::P => self.pin.set_active(!self.pin.is_active()),
            gdk::Key::i | gdk::Key::I => self.properties(),
            gdk::Key::t | gdk::Key::T => {
                let toggle = self.state.borrow().active.pdf_sidebar.clone();
                if let Some(toggle) = toggle {
                    toggle.set_active(!toggle.is_active());
                }
            }
            gdk::Key::r | gdk::Key::R => {
                if let Some(canvas) = &self.state.borrow().active.canvas {
                    canvas.rotate(!shift);
                }
            }
            gdk::Key::_0 => {
                if self.state.borrow().pdf_continuous {
                    self.reset_pdf_zoom();
                } else if let Some(canvas) = &self.state.borrow().active.canvas {
                    canvas.fit();
                }
            }
            gdk::Key::_1 => {
                if self.state.borrow().pdf_continuous {
                    self.reset_pdf_zoom();
                } else if let Some(canvas) = &self.state.borrow().active.canvas {
                    canvas.actual_size();
                }
            }
            gdk::Key::plus | gdk::Key::equal => {
                if self.state.borrow().pdf_continuous {
                    self.zoom_all_pages(1.2);
                } else if let Some(canvas) = &self.state.borrow().active.canvas {
                    canvas.zoom_by(1.2);
                }
            }
            gdk::Key::minus => {
                if self.state.borrow().pdf_continuous {
                    self.zoom_all_pages(1. / 1.2);
                } else if let Some(canvas) = &self.state.borrow().active.canvas {
                    canvas.zoom_by(1. / 1.2);
                }
            }
            gdk::Key::w | gdk::Key::W => {
                if let Some(view) = &self.state.borrow().active.text {
                    view.set_wrap_mode(if view.wrap_mode() == gtk4::WrapMode::None {
                        gtk4::WrapMode::WordChar
                    } else {
                        gtk4::WrapMode::None
                    });
                }
            }
            gdk::Key::e | gdk::Key::E => self.cycle_encoding(),
            gdk::Key::k | gdk::Key::K => {
                let s = self.state.borrow();
                if let Some(media) = &s.active.media {
                    media.command(&["cycle", "pause"]);
                } else if let Some(button) = &s.active.animation_pause {
                    button.set_active(!button.is_active());
                } else {
                    s.active.paused.set(!s.active.paused.get());
                }
            }
            gdk::Key::comma => {
                if let Some(media) = &self.state.borrow().active.media {
                    media.command(&["frame-back-step"]);
                }
            }
            gdk::Key::period => {
                if let Some(media) = &self.state.borrow().active.media {
                    media.command(&["frame-step"]);
                }
            }
            gdk::Key::less => {
                if let Some(media) = &self.state.borrow().active.media {
                    media.command(&["multiply", "speed", "0.8"]);
                }
            }
            gdk::Key::greater => {
                if let Some(media) = &self.state.borrow().active.media {
                    media.command(&["multiply", "speed", "1.25"]);
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }
    fn cycle_encoding(self: &Rc<Self>) {
        let (file, label) = {
            let s = self.state.borrow();
            let Some(prepared) = &s.prepared else {
                return;
            };
            let label = match &prepared.body {
                Body::Text(t)
                | Body::Markdown { source: t, .. }
                | Body::Structured { source: t, .. } => t.encoding.as_str(),
                Body::Csv(doc) => doc.source.encoding.as_str(),
                _ => return,
            };
            let labels = ["UTF-8", "GBK", "GB18030", "windows-1252"];
            let index = labels
                .iter()
                .position(|l| l.eq_ignore_ascii_case(label))
                .unwrap_or(0);
            (
                prepared.info.clone(),
                labels[(index + 1) % labels.len()].to_string(),
            )
        };
        self.load(
            file,
            Spec {
                encoding: Some(label),
                ..Default::default()
            },
        );
    }
    fn change_page(self: &Rc<Self>, delta: i32) {
        let (file, page) = {
            let s = self.state.borrow();
            if s.pages == 0 {
                return;
            }
            let Some(file) = s.current.clone() else {
                return;
            };
            (file, (s.page + delta).clamp(0, s.pages - 1))
        };
        self.load(
            file,
            Spec {
                page,
                ..Default::default()
            },
        );
    }
    /// Page Up / Page Down：单页模式翻页；连续模式里滚动到上一页/下一页，
    /// 不再切回单页（否则会被「默认连续滚动」立刻拉回顶部，看起来完全没反应）。
    fn pdf_page_step(self: &Rc<Self>, delta: i32) {
        let (continuous, pages) = {
            let s = self.state.borrow();
            (s.pdf_continuous, s.pages)
        };
        if pages == 0 {
            return;
        }
        if !continuous {
            self.change_page(delta);
            return;
        }
        let Some(scrolled) = self
            .state
            .borrow()
            .pdf_scroll
            .as_ref()
            .and_then(|w| w.upgrade())
        else {
            return;
        };
        let adj = scrolled.vadjustment();
        let span = (adj.upper() - adj.page_size()).max(adj.lower());
        // 一次翻一屏（略小于一屏，保留一点上下文），和常见 PDF 阅读器的
        // Page Up / Page Down 一致；不依赖每页高度，缩放后也不会漂移。
        let y = page_scroll_target(adj.value(), adj.page_size(), adj.upper(), delta);
        adj.set_value(y);
        let pct = if span > adj.lower() {
            ((y - adj.lower()) / (span - adj.lower()) * 100.).round() as i32
        } else {
            0
        };
        self.status.set_text(&format!("{pct}% · Page Up/Down 翻页"));
    }
    /// 当前文件对应的内容区背景色；`None` 表示跟随主题。
    fn background_color(&self, info: &FileInfo) -> Option<gdk::RGBA> {
        let value = match background_slot(info) {
            "pdf" => &self.config.background_pdf,
            "image" => &self.config.background_image,
            "video" => &self.config.background_video,
            "audio" => &self.config.background_audio,
            "office" => &self.config.background_office,
            "sheet" => &self.config.background_sheet,
            "text" => &self.config.background_text,
            _ => &self.config.background_other,
        };
        parse_color(value)
    }
    /// 非画布类视图（文本、列表等）用一张低优先级之上的 CSS 表换底色。
    fn apply_background(&self, info: &quicklook_core::files::FileInfo) {
        let css = match self.background_color(info) {
            Some(color) => {
                let value = color_text(&color);
                format!(
                    ".quicklook .preview-bg {{ background-color: {value}; }}\n                     .quicklook .preview-bg text {{ background-color: {value}; }}\n                     .quicklook .preview-bg listview, .quicklook .preview-bg gridview {{ background-color: {value}; }}"
                )
            }
            None => String::new(),
        };
        self.bg_provider.load_from_string(&css);
    }
    fn status_json(&self) -> serde_json::Value {
        let s = self.state.borrow();
        // Inside an archive the prepared file is what is on screen, so the
        // reported file follows the breadcrumb instead of the outer zip.
        let file = s
            .prepared
            .as_ref()
            .map(|p| p.info.clone())
            .or_else(|| s.current.clone());
        serde_json::json!({"version":quicklook_core::VERSION,"running":true,"picker_open":self.picker.borrow().is_some(),"visible":self.window.is_visible(),"backend":self.backend(),"app_id":APP_ID,"pinned":s.pinned,"detached":s.detached,"fullscreen":self.fullscreen.get(),"ready":s.ready,"grid":s.grid,"generation":s.generation,"file":file,"selection_count":s.selection.len(),"sequence_count":s.sequence.len(),"page":s.page+1,"pages":s.pages,"cache_entries":s.cache.len(),"cache_bytes":s.cache_bytes,"cache_hits":s.cache_hits,"request_to_content_ms":s.last_elapsed_ms,"error":s.last_error,"media":s.active.media.as_ref().map(MediaView::status),"image":s.active.canvas.as_ref().map(Canvas::detail_status),"animation_frames":s.active.animation_frames.get(),"animation_paused":s.active.paused.get(),"pdf_continuous":s.pdf_continuous,"pdf_user_mode":s.pdf_user_mode,"pdf_zoom":s.pdf_zoom,"pdf_thumbnails":s.pdf_thumbnails,"body":s.prepared.as_ref().map(|p|body_name(&p.body)).unwrap_or(""),"mermaid":s.mermaid,"nested_depth":s.nested.len(),"nested_path":s.nested.iter().map(|p|p.info.name.clone()).chain(s.prepared.as_ref().map(|p|p.info.name.clone())).collect::<Vec<_>>()})
    }
    fn error(&self, error: &str) {
        clear(&self.content);
        let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        box_.set_halign(gtk4::Align::Center);
        box_.set_valign(gtk4::Align::Center);
        box_.set_vexpand(true);
        let icon = gtk4::Image::from_icon_name("dialog-information-symbolic");
        icon.set_pixel_size(48);
        let title = gtk4::Label::new(Some("暂时无法预览此文件"));
        title.add_css_class("title-2");
        let label = gtk4::Label::new(Some(error));
        label.set_wrap(true);
        label.set_max_width_chars(75);
        label.set_selectable(true);
        box_.append(&icon);
        box_.append(&title);
        box_.append(&label);
        self.content.append(&box_);
        self.status.set_text("可查看属性，或使用默认应用打开");
        let mut s = self.state.borrow_mut();
        s.ready = true;
        s.last_error = error.into();
        s.last_elapsed_ms = s.request_start.elapsed().as_secs_f64() * 1000.;
        drop(s);
        if !self.window.is_visible() {
            self.present_preview();
        }
    }
}
/// 依据显示器尺寸计算预览窗口默认尺寸：上限是显示器几何的 70%。
///
/// `aspect` 是图片/PDF 页面的宽高比（`None` 表示普通文档，直接用设置里的尺寸）；
/// `configured` 是用户在设置里指定的固定尺寸，会按屏幕 70% 上限截断。
/// 把文件归类到设置里的一档背景色。常见格式各自对齐经典应用，
/// 其余（目录、压缩包、字体等）统一落到「其它」。
fn background_slot(info: &FileInfo) -> &'static str {
    let name = info.name.to_ascii_lowercase();
    let ext = name.rsplit('.').next().unwrap_or("");
    let audio = info.mime.starts_with("audio/")
        || matches!(ext, "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" | "aac");
    match info.kind {
        Kind::Pdf => "pdf",
        Kind::Image | Kind::Raw | Kind::Psd | Kind::Eps => "image",
        Kind::Media if audio => "audio",
        Kind::Media => "video",
        Kind::Office if matches!(ext, "xlsx" | "xls" | "ods") => "sheet",
        Kind::Office => "office",
        Kind::Text | Kind::Markdown | Kind::Html | Kind::Structured | Kind::Rtf
            if matches!(ext, "csv" | "tsv") =>
        {
            "sheet"
        }
        Kind::Text | Kind::Markdown | Kind::Html | Kind::Structured | Kind::Rtf => "text",
        _ => "other",
    }
}
/// 解析 `#RRGGBB` 配置色。
fn parse_color(value: &str) -> Option<gdk::RGBA> {
    let text = value.trim();
    let bytes = text.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| -> Option<f32> {
        let hex = std::str::from_utf8(&bytes[range]).ok()?;
        Some(u8::from_str_radix(hex, 16).ok()? as f32 / 255.)
    };
    Some(gdk::RGBA::new(
        channel(1..3)?,
        channel(3..5)?,
        channel(5..7)?,
        1.,
    ))
}
fn color_text(color: &gdk::RGBA) -> String {
    let channel = |value: f32| (value * 255.).round().clamp(0., 255.) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.red()),
        channel(color.green()),
        channel(color.blue())
    )
}

/// 空格是预览窗口的关闭键。Ctrl/Alt+空格留给窗口管理器或输入法，
/// 其余情况（包括焦点在页码 SpinButton 或按钮上）都必须关闭。
fn space_closes(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    key == gdk::Key::space
        && !modifiers.contains(gdk::ModifierType::CONTROL_MASK)
        && !modifiers.contains(gdk::ModifierType::ALT_MASK)
}

fn preview_size(
    monitor: (i32, i32),
    aspect: Option<f64>,
    configured: Option<(i32, i32)>,
) -> (i32, i32) {
    let maxw = (monitor.0 as f64 * 0.7) as i32;
    let maxh = (monitor.1 as f64 * 0.7) as i32;
    // 标题栏 + 格式栏 + 状态栏的实际高度；窗口按它给内容留位。
    const CHROME: i32 = 104;
    let (w, h) = if let Some(aspect) = aspect {
        let contenth = (maxh - CHROME).max(180);
        let w = (contenth as f64 * aspect) as i32;
        if w > maxw {
            (maxw, (maxw as f64 / aspect) as i32 + CHROME)
        } else {
            (w, contenth + CHROME)
        }
    } else {
        let (base_w, base_h) = configured.unwrap_or((940, 650));
        (base_w.min(maxw), base_h.min(maxh))
    };
    (w.max(320).min(maxw), h.max(260).min(maxh))
}

fn icon_button(icon: &str, tooltip: &str) -> gtk4::Button {
    gtk4::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}
fn clear(box_: &gtk4::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
}
fn texture(pixels: &crate::backend::Pixels) -> gdk::MemoryTexture {
    gdk::MemoryTexture::new(
        pixels.width as i32,
        pixels.height as i32,
        if pixels.bgra {
            gdk::MemoryFormat::B8g8r8a8Premultiplied
        } else {
            gdk::MemoryFormat::R8g8b8a8
        },
        &glib::Bytes::from_owned(pixels.bytes.clone()),
        pixels.width as usize * 4,
    )
}

/// 当前渲染的内容类型，供状态查询与自动验证使用。
pub fn body_name(body: &Body) -> &'static str {
    match body {
        Body::Tile { .. } => "tile",
        Body::Image(_) => "image",
        Body::Pdf(_) => "pdf",
        Body::Text(_) => "text",
        Body::Csv(_) => "csv",
        Body::Markdown { .. } => "markdown",
        Body::Rtf { .. } => "rtf",
        Body::Html(_) => "html",
        Body::Structured { .. } => "structured",
        Body::Directory(_) => "directory",
        Body::Archive { .. } => "archive",
        Body::Binary(_) => "binary",
        Body::Summary(_) => "summary",
        Body::Media => "media",
        Body::Office(_) => "office",
        Body::Font(_) => "font",
    }
}

/// 计算连续滚动模式下按 Page Up / Page Down 后应滚到的位置。
/// `delta` 为正表示向下翻一屏。结果夹在 `[0, upper - page_size]` 之间，
/// 因此到顶/到底时不会越界。
fn page_scroll_target(value: f64, page_size: f64, upper: f64, delta: i32) -> f64 {
    let span = (upper - page_size).max(0.);
    let step = page_size.max(80.) * 0.92;
    (value + delta as f64 * step).clamp(0., span)
}

#[cfg(test)]
mod page_scroll_tests {
    use super::page_scroll_target;

    #[test]
    fn page_down_moves_almost_one_viewport_down() {
        let y = page_scroll_target(0., 600., 6000., 1);
        assert!((y - 552.).abs() < 0.01, "y = {y}");
        assert_eq!(page_scroll_target(y, 600., 6000., 1), 1104.);
    }

    #[test]
    fn page_up_moves_back_and_clamps_at_top_and_bottom() {
        assert_eq!(page_scroll_target(1104., 600., 6000., -1), 552.);
        assert_eq!(page_scroll_target(0., 600., 6000., -1), 0.);
        assert_eq!(page_scroll_target(5400., 600., 6000., 1), 5400.);
    }
}

#[cfg(test)]
mod window_size_tests {
    use super::{preview_size, space_closes};
    use gtk4::gdk;

    #[test]
    fn space_closes_unless_control_or_alt_is_held() {
        assert!(space_closes(gdk::Key::space, gdk::ModifierType::empty()));
        assert!(space_closes(gdk::Key::space, gdk::ModifierType::SHIFT_MASK));
        assert!(!space_closes(
            gdk::Key::space,
            gdk::ModifierType::CONTROL_MASK
        ));
        assert!(!space_closes(gdk::Key::space, gdk::ModifierType::ALT_MASK));
        assert!(!space_closes(gdk::Key::Escape, gdk::ModifierType::empty()));
    }

    #[test]
    fn default_size_is_the_configured_nine_forty_by_six_fifty() {
        assert_eq!(preview_size((1920, 1080), None, None), (940, 650));
    }

    #[test]
    fn explicit_size_is_respected_but_capped_at_seventy_percent() {
        assert_eq!(
            preview_size((1920, 1080), None, Some((1120, 790))),
            (1120, 756)
        );
        assert_eq!(
            preview_size((1920, 1080), None, Some((2400, 1400))),
            (1344, 756)
        );
    }

    #[test]
    fn page_aspect_only_leaves_chrome_outside_the_content() {
        let (w, h) = preview_size((1920, 1080), Some(16.0 / 9.0), None);
        assert_eq!(h, 756);
        assert_eq!(h - 104, 652);
        assert!(w <= 1344, "w = {w}");
        assert!(w > 1100, "w = {w}");
    }

    #[test]
    fn a_small_monitor_still_gets_a_usable_window() {
        let (w, h) = preview_size((1024, 768), None, None);
        assert_eq!((w, h), (716, 537));
    }
}
