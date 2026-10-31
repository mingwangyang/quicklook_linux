//! 设置窗口：预览键（全局默认 + 逐宿主覆盖）、窗口默认布局、主题、
//! 是否劫持宿主原生预览、各文件管理器集成开关。
//!
//! 保存时先写 `config.toml`，再按开关把集成文件落到 XDG 目录，因此开关立即生效；
//! 重新安装二进制也不会把用户的选择改回去。
use anyhow::{Context, Result};
use gio::prelude::*;
use gtk4::prelude::*;
use quicklook_core::{
    config::{Config, config_path},
    hosts,
    keys::{KEY_NAMES, MODIFIER_NAMES},
};

const APP_ID: &str = "org.quicklook.Preview.Settings";
const ABOUT_APP_ID: &str = "org.quicklook.Preview.About";

/// 界面里每个可保存项都提供两个方向：把配置灌进控件、把控件写回配置。
type Restore = Box<dyn Fn(&Config)>;
type Capture = Box<dyn Fn(&mut Config)>;

#[derive(Default)]
struct Fields {
    restore: Vec<Restore>,
    capture: Vec<Capture>,
}
impl Fields {
    fn add(&mut self, restore: Restore, capture: Capture) {
        self.restore.push(restore);
        self.capture.push(capture);
    }
    fn fill(&self, config: &Config) {
        for restore in &self.restore {
            restore(config);
        }
    }
    fn read(&self) -> Config {
        let mut config = Config::load().unwrap_or_default();
        for capture in &self.capture {
            capture(&mut config);
        }
        config
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("quicklook-settings: {error:#}");
        std::process::exit(1);
    }
}

/// 托盘右键「关于」：一个只读的小窗口，写清名字与版本。
fn about() -> Result<()> {
    let app = gtk4::Application::new(Some(ABOUT_APP_ID), gio::ApplicationFlags::NON_UNIQUE);
    app.register(None::<&gio::Cancellable>)?;
    let _hold = app.hold();
    let window = gtk4::ApplicationWindow::builder()
        .application(&app)
        .title("关于 Quick Look")
        .resizable(false)
        .build();
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
    root.set_margin_top(22);
    root.set_margin_bottom(18);
    root.set_margin_start(28);
    root.set_margin_end(28);
    let icon = gtk4::Image::from_icon_name("org.quicklook.Preview");
    icon.set_pixel_size(72);
    root.append(&icon);
    let name = gtk4::Label::new(None);
    name.set_markup("<span size='x-large' weight='bold'>Quick Look</span>");
    root.append(&name);
    let version = gtk4::Label::new(Some(&format!(
        "版本 {}\n{}",
        quicklook_core::VERSION,
        "按空格即预览文件"
    )));
    version.set_justify(gtk4::Justification::Center);
    root.append(&version);
    let detail = gtk4::Label::new(Some(
        "原生 GTK4 预览器，支持图片、音视频、PDF、Office、压缩包内浏览、\nMarkdown（含 Mermaid 流程图）等格式。\n许可：GPL-3.0-or-later",
    ));
    detail.set_justify(gtk4::Justification::Center);
    detail.add_css_class("dim-label");
    root.append(&detail);
    let close = gtk4::Button::with_label("关闭");
    root.append(&close);
    window.set_child(Some(&root));
    let mainloop = glib::MainLoop::new(None, false);
    {
        let mainloop_quit = mainloop.clone();
        let target = window.clone();
        close.connect_clicked(move |_| target.close());
        window.connect_close_request(move |_| {
            mainloop_quit.quit();
            glib::Propagation::Proceed
        });
    }
    window.present();
    mainloop.run();
    Ok(())
}

fn run() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|arg| arg == "--help") {
        println!(
            "quicklook-settings\n调整统一预览键与各文件管理器集成；等价于 quicklook --settings\n--about 只显示版本信息（托盘右键「关于」用它）"
        );
        return Ok(());
    }
    let config = Config::load()?;
    gtk4::init()?;
    if arguments.iter().any(|arg| arg == "--about") {
        return about();
    }
    apply_theme(config.uses_dark_theme());
    let app = gtk4::Application::new(Some(APP_ID), gio::ApplicationFlags::NON_UNIQUE);
    app.register(None::<&gio::Cancellable>)?;
    let _hold = app.hold();
    let window = gtk4::ApplicationWindow::builder()
        .application(&app)
        .title(format!("Quick Look 设置 · {}", quicklook_core::VERSION))
        .default_width(600)
        .default_height(760)
        .build();
    let collector = std::rc::Rc::new(std::cell::RefCell::new(Fields::default()));
    let status = gtk4::Label::new(None);
    let close = build(&window, &config, &collector, &status);
    let mainloop = glib::MainLoop::new(None, false);
    {
        let mainloop_quit = mainloop.clone();
        let target = window.clone();
        close.connect_clicked(move |_| target.close());
        window.connect_close_request(move |_| {
            mainloop_quit.quit();
            glib::Propagation::Proceed
        });
    }
    window.present();
    mainloop.run();
    Ok(())
}

/// 构建界面；返回「关闭」按钮，供窗口关闭时一并结束主循环。
fn build(
    window: &gtk4::ApplicationWindow,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
    status: &gtk4::Label,
) -> gtk4::Button {
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    root.set_margin_top(10);
    root.set_margin_bottom(12);
    root.set_margin_start(14);
    root.set_margin_end(14);

    // 顶部一条常驻说明，直接回答「改完怎样才算生效」。
    let banner = gtk4::Label::new(Some(
        "左侧选择分类。所有改动都要点右下角「保存设置」才会写入并生效；\
         文件管理器接入选项还要再点一次「应用文件管理器接入 / 注入」。",
    ));
    banner.set_xalign(0.);
    banner.set_wrap(true);
    banner.add_css_class("settings-banner");
    root.append(&banner);

    let stack = gtk4::Stack::new();
    stack.set_vexpand(true);
    stack.set_hexpand(true);
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    push_page(&stack, "keys", "快捷键", |page| {
        key_section(page, config, collector)
    });
    push_page(&stack, "window", "窗口", |page| {
        window_section(page, config, collector)
    });
    push_page(&stack, "appearance", "外观", |page| {
        appearance_section(page, config, collector)
    });
    push_page(&stack, "hosts", "文件管理器", |page| {
        host_section(page, config, collector, status)
    });
    push_page(&stack, "render", "渲染与缓存", |page| {
        render_section(page, config, collector)
    });
    push_page(&stack, "startup", "启动", |page| {
        startup_section(page, config, collector)
    });

    let body = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    body.set_vexpand(true);
    let sidebar = gtk4::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_size_request(148, -1);
    sidebar.add_css_class("settings-sidebar");
    body.append(&sidebar);
    body.append(&stack);
    root.append(&body);

    status.set_xalign(0.);
    status.set_wrap(true);
    status.set_text(&format!("配置文件：{}", config_path().display()));
    root.append(status);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let reset = gtk4::Button::with_label("恢复默认");
    let close = gtk4::Button::with_label("关闭");
    let save = gtk4::Button::with_label("保存设置");
    save.add_css_class("suggested-action");
    buttons.append(&reset);
    buttons.append(&close);
    buttons.append(&save);
    root.append(&buttons);
    window.set_child(Some(&root));

    {
        let collector = collector.clone();
        let status = status.clone();
        save.connect_clicked(move |_| {
            let config = collector.borrow().read();
            let text = match apply(&config, false) {
                Ok(text) => format!("✔ 已保存并生效。\n{text}"),
                Err(error) => format!("保存失败：{error:#}"),
            };
            status.set_text(&text);
        });
    }
    {
        let collector = collector.clone();
        let status = status.clone();
        let reset_config = Config::default();
        reset.connect_clicked(move |_| {
            collector.borrow().fill(&reset_config);
            status.set_text(
                "已恢复默认选项，还需点「保存设置」写入；文件管理器接入请再点「应用文件管理器接入 / 注入」。",
            );
        });
    }
    close
}

/// 生成一个分类页：带内边距的可滚动容器，登记到侧栏。
fn push_page<F>(stack: &gtk4::Stack, name: &str, title: &str, build_page: F)
where
    F: FnOnce(&gtk4::Box),
{
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
    page.set_margin_top(14);
    page.set_margin_bottom(14);
    page.set_margin_start(16);
    page.set_margin_end(16);
    build_page(&page);
    let scroll = gtk4::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&page)
        .build();
    stack.add_titled(&scroll, Some(name), title);
}

fn key_section(
    root: &gtk4::Box,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
) {
    let key = config.key();
    let inner = section(root, "预览快捷键");
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let names: Vec<&str> = KEY_NAMES.to_vec();
    let dropdown = gtk4::DropDown::from_strings(&names);
    dropdown.set_selected(
        KEY_NAMES
            .iter()
            .position(|name| *name == key.name())
            .unwrap_or(0) as u32,
    );
    row.append(&gtk4::Label::new(Some("全局默认")));
    row.append(&dropdown);
    let mut checks = Vec::new();
    for (index, name) in MODIFIER_NAMES.iter().enumerate() {
        let check = gtk4::CheckButton::with_label(name);
        check.set_active(match index {
            0 => key.ctrl(),
            1 => key.shift(),
            _ => key.alt(),
        });
        row.append(&check);
        checks.push(check);
    }
    inner.append(&row);
    let restore = {
        let dropdown = dropdown.clone();
        let checks = checks.clone();
        Box::new(move |source: &Config| {
            let key = source.key();
            dropdown.set_selected(
                KEY_NAMES
                    .iter()
                    .position(|name| *name == key.name())
                    .unwrap_or(0) as u32,
            );
            for (index, active) in [key.ctrl(), key.shift(), key.alt()].iter().enumerate() {
                if let Some(check) = checks.get(index) {
                    check.set_active(*active);
                }
            }
        })
    };
    let capture = {
        let dropdown = dropdown.clone();
        Box::new(move |target: &mut Config| {
            let name = KEY_NAMES[dropdown.selected() as usize % KEY_NAMES.len()];
            let key = quicklook_core::keys::PreviewKey::from_parts(
                name,
                checks[0].is_active(),
                checks[1].is_active(),
                checks[2].is_active(),
            );
            target.preview_key = key.unwrap_or_default().to_string();
        })
    };
    collector.borrow_mut().add(restore, capture);

    host_key_row(
        &inner,
        "Nemo",
        &config.preview_key_nemo,
        collector,
        |source| source.preview_key_nemo.clone(),
        |target, value| target.preview_key_nemo = value,
    );
    host_key_row(
        &inner,
        "Thunar",
        &config.preview_key_thunar,
        collector,
        |source| source.preview_key_thunar.clone(),
        |target, value| target.preview_key_thunar = value,
    );
    host_key_row(
        &inner,
        "Caja",
        &config.preview_key_caja,
        collector,
        |source| source.preview_key_caja.clone(),
        |target, value| target.preview_key_caja = value,
    );
    host_key_row(
        &inner,
        "Yazi",
        &config.preview_key_yazi,
        collector,
        |source| source.preview_key_yazi.clone(),
        |target, value| target.preview_key_yazi = value,
    );
    host_key_row(
        &inner,
        "Nautilus",
        &config.preview_key_nautilus,
        collector,
        |source| source.preview_key_nautilus.clone(),
        |target, value| target.preview_key_nautilus = value,
    );
    inner.append(&hint(&format!(
        "当前全局默认：{}。选「跟随全局默认」的宿主用上面的按键，也可以各自指定。\
         Nautilus 的空格走原生预览接口，Ctrl/Alt 组合走脚本加速键；\
         Dolphin 23.08 仅右键；26.04+（KIO 6.24+）可在其「配置快捷键」中把 Quick Look 设为空格。把裸空格改成别的键时，\
         Nemo / Thunar / Caja 里原来的空格会被旁路器吃掉，避免同时弹出两套预览。\
         保存后重启对应的文件管理器才会生效。",
        key.describe()
    )));
}

/// 单个宿主的覆盖按键行：下拉第一项是「跟随全局默认」，选中它时修饰键不可用。
fn host_key_row(
    inner: &gtk4::Box,
    label: &str,
    current: &str,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
    read: impl Fn(&Config) -> String + 'static,
    write: impl Fn(&mut Config, String) + 'static,
) {
    let mut options: Vec<String> = vec!["跟随全局默认".into()];
    options.extend(KEY_NAMES.iter().map(|name| (*name).to_string()));
    let names: Vec<&str> = options.iter().map(String::as_str).collect();
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let dropdown = gtk4::DropDown::from_strings(&names);
    row.append(&gtk4::Label::new(Some(label)));
    row.append(&dropdown);
    let mut checks = Vec::new();
    for name in MODIFIER_NAMES {
        let check = gtk4::CheckButton::with_label(name);
        row.append(&check);
        checks.push(check);
    }
    inner.append(&row);
    set_host_key_widgets(&dropdown, &checks, current);
    {
        let dropdown = dropdown.clone();
        let checks = checks.clone();
        dropdown
            .clone()
            .connect_notify_local(Some("selected-item"), move |_, _| {
                let following = dropdown.selected() == 0;
                for check in checks.iter() {
                    check.set_sensitive(!following);
                }
            });
    }
    let restore = {
        let dropdown = dropdown.clone();
        let checks = checks.clone();
        Box::new(move |source: &Config| {
            set_host_key_widgets(&dropdown, &checks, &read(source));
        })
    };
    let capture = {
        let dropdown = dropdown.clone();
        let checks = checks.clone();
        Box::new(move |target: &mut Config| {
            write(target, host_key_value(&dropdown, &checks));
        })
    };
    collector.borrow_mut().add(restore, capture);
}

fn set_host_key_widgets(dropdown: &gtk4::DropDown, checks: &[gtk4::CheckButton], value: &str) {
    let key = (!value.trim().is_empty())
        .then(|| quicklook_core::keys::PreviewKey::parse(value))
        .flatten();
    match key {
        Some(key) => {
            let index = KEY_NAMES
                .iter()
                .position(|name| *name == key.name())
                .unwrap_or(0);
            dropdown.set_selected(1 + index as u32);
            for (position, check) in checks.iter().enumerate() {
                check.set_active(match position {
                    0 => key.ctrl(),
                    1 => key.shift(),
                    _ => key.alt(),
                });
            }
        }
        None => {
            dropdown.set_selected(0);
            for check in checks {
                check.set_active(false);
            }
        }
    }
}

fn host_key_value(dropdown: &gtk4::DropDown, checks: &[gtk4::CheckButton]) -> String {
    let selected = dropdown.selected();
    if selected == 0 {
        return String::new();
    }
    let Some(name) = KEY_NAMES.get((selected - 1) as usize) else {
        return String::new();
    };
    quicklook_core::keys::PreviewKey::from_parts(
        name,
        checks.first().is_some_and(|c| c.is_active()),
        checks.get(1).is_some_and(|c| c.is_active()),
        checks.get(2).is_some_and(|c| c.is_active()),
    )
    .map(|key| key.to_string())
    .unwrap_or_default()
}

fn startup_section(
    root: &gtk4::Box,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
) {
    let inner = section(root, "启动");
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let toggle = gtk4::Switch::new();
    toggle.set_active(config.autostart);
    toggle.set_valign(gtk4::Align::Center);
    row.append(&gtk4::Label::new(Some("登录时自动启动常驻服务")));
    row.append(&toggle);
    inner.append(&row);
    inner.append(&hint(
        "常驻服务负责通知栏图标，并把第一次预览预热到几十毫秒；预览窗口本身仍然按需\n\
         打开、用完自动退出。关闭后第一次按预览键时服务会自动拉起。\n\
         写入 ~/.config/autostart/org.quicklook.Preview.desktop，下次登录生效。",
    ));
    let restore = {
        let toggle = toggle.clone();
        Box::new(move |source: &Config| toggle.set_active(source.autostart))
    };
    let capture = {
        let toggle = toggle.clone();
        Box::new(move |target: &mut Config| target.autostart = toggle.is_active())
    };
    collector.borrow_mut().add(restore, capture);
}

fn window_section(
    root: &gtk4::Box,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
) {
    let inner = section(root, "窗口布局");
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let dropdown = gtk4::DropDown::from_strings(&["浮动（默认）", "平铺"]);
    dropdown.set_selected(if config.uses_floating_layout() { 0 } else { 1 });
    row.append(&gtk4::Label::new(Some("默认布局")));
    row.append(&dropdown);
    inner.append(&row);
    inner.append(&hint(
        "下一次打开预览窗口时生效；已经打开的窗口可用 niri 的切换浮动快捷键临时调整。\
         独立窗口（Ctrl+D）始终按普通窗口平铺。默认布局由 niri 的窗口规则实现，\
         需要安装时使用过 --with-niri。",
    ));
    let restore = {
        let dropdown = dropdown.clone();
        Box::new(move |source: &Config| {
            dropdown.set_selected(if source.uses_floating_layout() { 0 } else { 1 });
        })
    };
    let capture = {
        let dropdown = dropdown.clone();
        Box::new(move |target: &mut Config| {
            target.window_mode = if dropdown.selected() == 1 {
                "tiled".into()
            } else {
                "floating".into()
            };
        })
    };
    collector.borrow_mut().add(restore, capture);

    // 默认窗口大小：宽度、高度各一个 SpinButton；「按内容自动」时存 0。
    let size = section(root, "默认窗口大小");
    let auto_start = config.window_width == 0 || config.window_height == 0;
    let (width0, height0) = config
        .window_size()
        .map(|(w, h)| (w as f64, h as f64))
        .unwrap_or((940., 650.));
    let size_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let width = gtk4::SpinButton::with_range(480., 7680., 20.);
    width.set_value(width0);
    width.set_width_chars(6);
    let height = gtk4::SpinButton::with_range(320., 4320., 20.);
    height.set_value(height0);
    height.set_width_chars(6);
    size_row.append(&gtk4::Label::new(Some("宽")));
    size_row.append(&width);
    size_row.append(&gtk4::Label::new(Some("×  高")));
    size_row.append(&height);
    size.append(&size_row);
    let auto = gtk4::CheckButton::with_label("按内容自动（不固定尺寸）");
    auto.set_active(auto_start);
    size.append(&auto);
    width.set_sensitive(!auto_start);
    height.set_sensitive(!auto_start);
    {
        let width = width.clone();
        let height = height.clone();
        auto.connect_toggled(move |check| {
            width.set_sensitive(!check.is_active());
            height.set_sensitive(!check.is_active());
        });
    }
    size.append(&hint(
        "设置预览窗口打开时的初始大小（逻辑像素）。默认 940×650，会受屏幕可用区域限制，\
         不会超出屏幕。改完点「保存设置」，下一次打开预览窗口时生效。\
         勾选「按内容自动」则不固定尺寸，由每类内容自行决定（图片/PDF 按比例铺满可用空间）。",
    ));
    let size_restore = {
        let width = width.clone();
        let height = height.clone();
        let auto = auto.clone();
        Box::new(move |source: &Config| {
            let autofit = source.window_width == 0 || source.window_height == 0;
            auto.set_active(autofit);
            let (w, h) = source
                .window_size()
                .map(|(w, h)| (w as f64, h as f64))
                .unwrap_or((940., 650.));
            width.set_value(w);
            height.set_value(h);
            width.set_sensitive(!autofit);
            height.set_sensitive(!autofit);
        })
    };
    let size_capture = {
        let width = width.clone();
        let height = height.clone();
        let auto = auto.clone();
        Box::new(move |target: &mut Config| {
            if auto.is_active() {
                target.window_width = 0;
                target.window_height = 0;
            } else {
                target.window_width = width.value() as u32;
                target.window_height = height.value() as u32;
            }
        })
    };
    collector.borrow_mut().add(size_restore, size_capture);
}

fn appearance_section(
    root: &gtk4::Box,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
) {
    let inner = section(root, "外观");
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let dropdown = gtk4::DropDown::from_strings(&["浅色", "深色"]);
    dropdown.set_selected(if config.uses_dark_theme() { 1 } else { 0 });
    row.append(&gtk4::Label::new(Some("主题")));
    row.append(&dropdown);
    inner.append(&row);
    inner.append(&hint(
        "默认浅色。保存后设置窗口立即切换，之后打开的预览窗口使用新主题；已打开的预览窗口重新打开后生效。",
    ));
    let mermaid = gtk4::Switch::new();
    mermaid.set_active(config.mermaid);
    mermaid.set_halign(gtk4::Align::Start);
    let flow = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    flow.append(&mermaid);
    flow.append(&gtk4::Label::new(Some("渲染 Markdown 里的 Mermaid 流程图")));
    inner.append(&flow);
    inner.append(&hint(
        "默认开启。只对含 ```mermaid 代码块的 Markdown 生效：这类视图会打开 WebKit 的 JavaScript 并注入内置 Mermaid（离线，不联网）。其它视图仍然完全关闭 JavaScript。关闭后流程图按普通代码块显示。",
    ));
    let mermaid_restore = {
        let mermaid = mermaid.clone();
        Box::new(move |source: &Config| mermaid.set_active(source.mermaid))
    };
    let mermaid_capture = {
        let mermaid = mermaid.clone();
        Box::new(move |target: &mut Config| target.mermaid = mermaid.is_active())
    };
    collector.borrow_mut().add(mermaid_restore, mermaid_capture);
    let restore = {
        let dropdown = dropdown.clone();
        Box::new(move |source: &Config| {
            dropdown.set_selected(if source.uses_dark_theme() { 1 } else { 0 });
        })
    };
    let capture = {
        let dropdown = dropdown.clone();
        Box::new(move |target: &mut Config| {
            target.theme = if dropdown.selected() == 1 {
                "dark".into()
            } else {
                "light".into()
            };
        })
    };
    collector.borrow_mut().add(restore, capture);
}

fn host_section(
    root: &gtk4::Box,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
    status: &gtk4::Label,
) {
    let inner = section(root, "文件管理器集成");
    let hijack = gtk4::Switch::new();
    hijack.set_active(config.hijack_native);
    hijack.set_halign(gtk4::Align::Start);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.append(&hijack);
    row.append(&gtk4::Label::new(Some("启用预览按键接管（可选）")));
    inner.append(&row);
    inner.append(&hint(
        "默认关闭。Nemo / Nautilus 使用预览接口，Yazi 修改按键配置。Caja 安装 python3-caja 后可从普通命令加载 Rust 适配；Thunar 和改键后的 Nemo 使用适配启动器。关闭时保留右键入口。选择后点击下面的应用按钮；已运行的文件管理器不会被自动重启。",
    ));
    let mut hosts = Vec::new();
    for (index, label, active) in [
        (0usize, "Nemo（Cinnamon / Linux Mint）", config.host_nemo),
        (1, "Nautilus（GNOME 文件）", config.host_nautilus),
        (
            2,
            "Dolphin（KDE；新版可在宿主中绑定空格）",
            config.host_dolphin,
        ),
        (3, "Thunar（XFCE）", config.host_thunar),
        (4, "Caja（MATE）", config.host_caja),
        (5, "Yazi（终端）", config.host_yazi),
    ] {
        let check = gtk4::CheckButton::with_label(label);
        check.set_active(active);
        inner.append(&check);
        hosts.push((index, check));
    }
    let restore = {
        let hijack = hijack.clone();
        let hosts = hosts.clone();
        Box::new(move |source: &Config| {
            hijack.set_active(source.hijack_native);
            for (index, check) in &hosts {
                check.set_active(match index {
                    0 => source.host_nemo,
                    1 => source.host_nautilus,
                    2 => source.host_dolphin,
                    3 => source.host_thunar,
                    4 => source.host_caja,
                    _ => source.host_yazi,
                });
            }
        })
    };
    // 给「自动检测」按钮留一份句柄，capture 闭包会把原句柄移走。
    let hijack_detect = hijack.clone();
    let hosts_detect = hosts.clone();
    let capture = Box::new(move |target: &mut Config| {
        target.hijack_native = hijack.is_active();
        for (index, check) in &hosts {
            let value = check.is_active();
            match index {
                0 => target.host_nemo = value,
                1 => target.host_nautilus = value,
                2 => target.host_dolphin = value,
                3 => target.host_thunar = value,
                4 => target.host_caja = value,
                _ => target.host_yazi = value,
            }
        }
    });
    collector.borrow_mut().add(restore, capture);
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let detect = gtk4::Button::with_label("自动检测并启用");
    let install = gtk4::Button::with_label("应用文件管理器接入 / 注入");
    let remove = gtk4::Button::with_label("撤销全部接入");
    buttons.append(&detect);
    buttons.append(&install);
    buttons.append(&remove);
    inner.append(&buttons);
    {
        let hijack = hijack_detect.clone();
        let hosts = hosts_detect.clone();
        let status = status.clone();
        detect.connect_clicked(move |_| {
            let found = hosts::detect_hosts();
            if found.is_empty() {
                status.set_text(
                    "没有检测到常见的文件管理器（nemo / nautilus / dolphin / thunar / caja / yazi）。",
                );
                return;
            }
            hijack.set_active(true);
            for (index, check) in &hosts {
                let key = match index {
                    0 => "nemo",
                    1 => "nautilus",
                    2 => "dolphin",
                    3 => "thunar",
                    4 => "caja",
                    _ => "yazi",
                };
                check.set_active(found.contains(&key));
            }
            status.set_text(&format!(
                "检测到：{}。已自动勾选并开启接管，点「应用文件管理器接入 / 注入」写入。",
                found.join("、")
            ));
        });
    }
    {
        let collector = collector.clone();
        let status = status.clone();
        install.connect_clicked(move |_| {
            status.set_text(&match apply(&collector.borrow().read(), true) {
                Ok(text) => text,
                Err(error) => format!("接入未完成：{error:#}"),
            });
        });
    }
    {
        let collector = collector.clone();
        let status = status.clone();
        remove.connect_clicked(move |_| {
            let mut config = collector.borrow().read();
            config.hijack_native = false;
            config.host_nemo = false;
            config.host_nautilus = false;
            config.host_dolphin = false;
            config.host_thunar = false;
            config.host_caja = false;
            config.host_yazi = false;
            status.set_text(&match apply(&config, true) {
                Ok(text) => {
                    collector.borrow().fill(&config);
                    text
                }
                Err(error) => format!("撤销未完成：{error:#}"),
            });
        });
    }
}

fn render_section(
    root: &gtk4::Box,
    config: &Config,
    collector: &std::rc::Rc<std::cell::RefCell<Fields>>,
) {
    let inner = section(root, "渲染与缓存");
    let mut spins = Vec::new();
    for (index, label, value, min, max, step) in [
        (
            0usize,
            "图片最大边（像素）",
            config.image_max_edge as f64,
            256.,
            8192.,
            128.,
        ),
        (1, "缓存上限（MiB）", config.cache_mb as f64, 8., 2048., 16.),
        (
            2,
            "窗口空闲退出（秒）",
            config.viewer_grace_secs as f64,
            1.,
            30.,
            1.,
        ),
    ] {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let spin = gtk4::SpinButton::with_range(min, max, step);
        spin.set_value(value);
        row.append(&gtk4::Label::new(Some(label)));
        row.append(&spin);
        inner.append(&row);
        spins.push((index, spin));
    }
    let pdf_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let pdf = gtk4::Switch::new();
    pdf.set_active(config.pdf_continuous);
    pdf.set_valign(gtk4::Align::Center);
    pdf_row.append(&gtk4::Label::new(Some("PDF 默认连续滚动")));
    pdf_row.append(&pdf);
    inner.append(&pdf_row);
    inner.append(&hint(
        "默认开启：打开 PDF 直接进入连续滚动（多页纵向排列，Ctrl+滚轮或 +/- 缩放时所有页一起放大）。\
         取消勾选则默认单页显示，可在预览窗口的格式栏随时切换。",
    ));
    let pdf_restore = {
        let pdf = pdf.clone();
        Box::new(move |source: &Config| pdf.set_active(source.pdf_continuous))
    };
    let pdf_capture = {
        let pdf = pdf.clone();
        Box::new(move |target: &mut Config| target.pdf_continuous = pdf.is_active())
    };
    collector.borrow_mut().add(pdf_restore, pdf_capture);
    let restore = {
        let spins = spins.clone();
        Box::new(move |source: &Config| {
            for (index, spin) in &spins {
                spin.set_value(match index {
                    0 => source.image_max_edge as f64,
                    1 => source.cache_mb as f64,
                    _ => source.viewer_grace_secs as f64,
                });
            }
        })
    };
    let capture = Box::new(move |target: &mut Config| {
        for (index, spin) in &spins {
            let value = spin.value() as u32;
            match index {
                0 => target.image_max_edge = value,
                1 => target.cache_mb = value as usize,
                _ => target.viewer_grace_secs = value as u64,
            }
        }
    });
    collector.borrow_mut().add(restore, capture);
}

fn apply(config: &Config, apply_hosts: bool) -> Result<String> {
    apply_theme(config.uses_dark_theme());
    let path = config.save().context("写入配置失败")?;
    let mut text = format!("已保存到 {}。", path.display());
    // 自启条目不属于文件管理器接入，保存即对齐，省得再多一个「应用」按钮。
    match hosts::apply_autostart_only(config) {
        Ok(report) if !report.written.is_empty() => {
            text.push_str(&format!(
                "\n已开启登录自启（{}）。",
                report.written[0].display()
            ));
        }
        Ok(report) if !report.removed.is_empty() => {
            text.push_str("\n已关闭登录自启。");
        }
        Ok(_) => {}
        Err(error) => text.push_str(&format!("\n自启条目更新失败：{error:#}")),
    }
    if apply_hosts {
        let report =
            hosts::apply(config).context("应用文件管理器集成失败（已保存选择，可重试）")?;
        text.push_str(&format!("\n{}。", report.summary()));
        if !report.notes.is_empty() {
            text.push('\n');
            text.push_str(&report.notes.join("\n"));
        }
        text.push_str("\n请退出并重新打开文件管理器以加载更新。Caja 已安装 python3-caja 时可直接运行 caja；未安装时可用 quicklook --launch-host caja。Thunar / 改键后的 Nemo 请从应用菜单或适配启动器打开。");
    } else {
        text.push_str("\n文件管理器接入选项需另点「应用文件管理器接入 / 注入」。");
    }
    // 「应用接入」里已经维护过 niri 规则（hosts::apply 负责安装/更新），
    // 这里只在没勾选接入时补一次更新，避免重复写入与重复报账。
    let niri_sync = if apply_hosts {
        None
    } else {
        Some(quicklook_core::niri::sync(config.uses_floating_layout()))
    };
    match niri_sync {
        None => text.push_str("\nniri 窗口规则已随文件管理器接入一并更新。"),
        Some(Ok(Some(path))) => {
            let reloaded = quicklook_core::niri::reload();
            text.push_str(&format!(
                "\nniri 窗口规则已更新（{}）{}。",
                path.display(),
                if reloaded {
                    "并已重新加载"
                } else {
                    "；niri 未运行，下次登录时生效"
                }
            ));
        }
        Some(Ok(None)) => text.push_str(
            "\n未找到 niri 窗口规则；点「应用文件管理器接入」可重新生成（需要 ~/.config/niri/config.kdl）。",
        ),
        Some(Err(error)) => text.push_str(&format!("\nniri 窗口规则更新失败：{error:#}")),
    }
    Ok(text)
}

fn apply_theme(dark: bool) {
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
}

fn section(root: &gtk4::Box, title: &str) -> gtk4::Box {
    let label = gtk4::Label::new(None);
    label.set_markup(&format!("<b>{title}</b>"));
    label.set_xalign(0.);
    root.append(&label);
    let inner = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    inner.set_margin_start(4);
    root.append(&inner);
    inner
}

fn hint(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.add_css_class("dim-label");
    label
}
