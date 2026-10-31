//! Resident GIO broker. GTK/WebKit/mpv are confined to a disposable viewer process.
mod tray;
use anyhow::{Context, Result, bail};
use glib::variant::ToVariant;
use quicklook_core::{
    config::{Config, cache_dir},
    protocol::*,
};
use std::{
    cell::{Cell, RefCell},
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

struct Service {
    connection: gio::DBusConnection,
    child: RefCell<Option<Child>>,
    last_action: RefCell<Instant>,
    loop_: glib::MainLoop,
    config: RefCell<Config>,
    nemo_owned: Cell<bool>,
}
impl Service {
    fn viewer(&self, method: &str, parameters: Option<&glib::Variant>) -> Result<glib::Variant> {
        Ok(self.connection.call_sync(
            Some(VIEWER_BUS),
            VIEWER_PATH,
            VIEWER_BUS,
            method,
            parameters,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            5000,
            None::<&gio::Cancellable>,
        )?)
    }
    fn start_viewer(&self) -> Result<()> {
        if self.viewer("Status", None).is_ok() {
            return Ok(());
        }
        let path = std::env::current_exe()?
            .parent()
            .context("无法定位服务目录")?
            .join("quicklook-viewer");
        std::fs::create_dir_all(cache_dir())?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(cache_dir().join("viewer.log"))?;
        let mut command = Command::new(&path);
        command
            .arg(format!(
                "--idle-exit-ms={}",
                self.config.borrow().viewer_grace_secs * 1000
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log));
        prefer_wayland(&mut command);
        let child = command
            .spawn()
            .with_context(|| format!("无法启动 {}", path.display()))?;
        if let Some(mut previous) = self.child.borrow_mut().replace(child) {
            let _ = previous.try_wait();
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if self.viewer("Status", None).is_ok() {
                return Ok(());
            }
            if let Some(child) = self.child.borrow_mut().as_mut()
                && let Some(status) = child.try_wait()?
            {
                bail!("预览进程退出（{status}）；查看 ~/.cache/quicklook/viewer.log");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        bail!("预览进程启动超时")
    }
    fn update_nemo(&self, force: bool) -> Result<()> {
        let config = self.config.borrow();
        let wanted = force || (config.host_nemo && config.hijack_native) || config.nemo_compat;
        if wanted && !self.nemo_owned.get() {
            self.nemo_owned
                .set(request_name(&self.connection, NEMO_BUS)?);
        } else if !wanted && self.nemo_owned.get() {
            self.connection.call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ReleaseName",
                Some(&(NEMO_BUS,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                3000,
                None::<&gio::Cancellable>,
            )?;
            self.nemo_owned.set(false);
        }
        Ok(())
    }
    fn handle(&self, method: &str, parameters: &glib::Variant) -> Result<Option<glib::Variant>> {
        if method != "Status" {
            self.last_action.replace(Instant::now());
        }
        if method == "ReloadConfig" {
            let (enabled,) = parameters.get::<(bool,)>().context("无效接管状态")?;
            let mut config = Config::load()?;
            config.host_nemo = enabled;
            config.nemo_compat = false;
            self.config.replace(config);
            self.update_nemo(false)?;
            return Ok(None);
        }
        if method == "Quit" {
            let _ = self.viewer("Quit", None);
            self.loop_.quit();
            return Ok(None);
        }
        if method == "Status" {
            let mut value = if let Ok(reply) = self.viewer("Status", None) {
                let (text,) = reply.get::<(String,)>().context("无效预览状态")?;
                let mut value = serde_json::from_str::<serde_json::Value>(&text)?;
                value["viewer_running"] = true.into();
                value
            } else {
                serde_json::json!({"running":true,"visible":false,"ready":false,"cache_bytes":0,"viewer_running":false})
            };
            value["version"] = quicklook_core::VERSION.into();
            value["service_pid"] = std::process::id().into();
            if let Some(child) = self.child.borrow().as_ref() {
                value["viewer_pid"] = child.id().into();
            }
            return Ok(Some((value.to_string(),).to_variant()));
        }
        if matches!(method, "Preview" | "Show" | "Toggle" | "Pick") {
            self.start_viewer()?;
        } else if method == "SetSelection" {
            let visible = self
                .viewer("Status", None)
                .ok()
                .and_then(|v| v.get::<(String,)>())
                .and_then(|(s,)| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v["visible"].as_bool())
                .unwrap_or(false);
            if !visible {
                return Ok(None);
            }
        }
        match self.viewer(method, Some(parameters)) {
            Ok(reply) => Ok(Some(reply)),
            Err(error) if matches!(method, "Close" | "Next" | "Prev" | "Pin") => {
                let _ = error;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}
fn request_name(connection: &gio::DBusConnection, name: &str) -> Result<bool> {
    let result = connection.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&(name, 4u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        None::<&gio::Cancellable>,
    )?;
    Ok(result
        .get::<(u32,)>()
        .map(|(c,)| c == 1 || c == 4)
        .unwrap_or(false))
}
/// 重启时新进程会和旧进程短暂并存：旧进程退出前仍持有总线名，RequestName
/// 会立刻返回 EXISTS。这里只在新进程里轮询等待，直到旧连接释放名字。
fn request_name_retry(
    connection: &gio::DBusConnection,
    name: &str,
    timeout: Duration,
) -> Result<bool> {
    let start = Instant::now();
    loop {
        if request_name(connection, name)? {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
/// Launches a sibling binary from the same install prefix. The tray has no
/// window to parent these to, so they are fully detached.
fn spawn(name: &str, args: &[&str]) {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let Some(directory) = executable.parent() else {
        return;
    };
    let path = directory.join(name);
    let mut command = Command::new(&path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    prefer_wayland(&mut command);
    if let Err(error) = command.spawn() {
        eprintln!("quicklookd: 无法启动 {}：{error}", path.display());
    }
}

/// 终端或 IDE 有时会导出 `GDK_BACKEND=x11`。XWayland 窗口在 niri 里没有 app-id，
/// 浮动/焦点规则全部匹配不到，预览就会平铺并且抢不到按键。Wayland 会话里明确
/// 回到默认后端，让窗口拿到 org.quicklook.Preview 这个 app-id。
fn prefer_wayland(command: &mut Command) {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        command.env_remove("GDK_BACKEND");
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("quicklookd: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    use gio::prelude::*;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help") {
        println!("quicklookd [--nemo-compat] [--replace]\nResident native DBus preview service");
        return Ok(());
    }
    if args
        .iter()
        .any(|a| a != "--nemo-compat" && a != "--replace")
    {
        bail!("未知参数");
    }
    let replace = args.iter().any(|a| a == "--replace");
    let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    let loop_ = glib::MainLoop::new(None, false);
    let config = Config::load()?;
    match quicklook_core::niri::sync(config.uses_floating_layout()) {
        Ok(Some(_)) => {
            quicklook_core::niri::reload();
        }
        Ok(None) => {}
        Err(error) => eprintln!("quicklookd: 无法同步 niri 窗口规则：{error:#}"),
    }
    // Extracted files must outlive a viewer (default-app opening and clipboard).
    // Only our private staging directories older than a day are reclaimed.
    let staging = cache_dir().join("extracted");
    if let Ok(entries) = std::fs::read_dir(staging) {
        for entry in entries.flatten().take(10_000) {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("quicklook-")
                && entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .map(|t| t > Duration::from_secs(86400))
                    .unwrap_or(false)
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    // Nemo 的空格由宿主自己处理：它调用 org.nemo.Preview.ShowFile，而不是把按键
    // 交给插件。因此接管原生预览总线即可，不依赖 LD_PRELOAD，也就不会受
    // 「Nemo 已经以旧实例在运行」影响。
    let nemo = config.nemo_compat
        || (config.host_nemo && config.hijack_native)
        || args.iter().any(|a| a == "--nemo-compat");
    let state = Rc::new(Service {
        connection: connection.clone(),
        child: RefCell::new(None),
        last_action: RefCell::new(Instant::now()),
        loop_: loop_.clone(),
        config: RefCell::new(config),
        nemo_owned: Cell::new(false),
    });
    let node = gio::DBusNodeInfo::for_xml(XML)?;
    let interface = node.lookup_interface(BUS).context("无效协议")?;
    let controller = state.clone();
    let _object = connection
        .register_object(PATH, &interface)
        .method_call(move |_, _, _, _, method, parameters, invocation| {
            match controller.handle(method, &parameters) {
                Ok(reply) => invocation.return_value(reply.as_ref()),
                Err(error) => invocation
                    .return_dbus_error("org.quicklook.Error.Failed", &format!("{error:#}")),
            }
        })
        .build()?;
    if !(if replace {
        request_name_retry(&connection, BUS, Duration::from_secs(10))?
    } else {
        request_name(&connection, BUS)?
    }) {
        return Ok(());
    }
    let tray_actions = tray::Actions {
        activate: Rc::new(|| spawn("quicklook", &["--pick"])),
        settings: Rc::new(|| spawn("quicklook-settings", &[])),
        about: Rc::new(|| spawn("quicklook-settings", &["--about"])),
        restart: {
            let state = state.clone();
            Rc::new(move || {
                // 新进程先起、旧进程后退出：新进程会等旧连接释放总线名，
                // 这样托盘和 Nemo 兼容接口在重启期间只会短暂中断。
                spawn("quicklookd", &["--replace"]);
                let _ = state.handle("Quit", &().to_variant());
            })
        },
        quit: {
            let state = state.clone();
            Rc::new(move || {
                let _ = state.handle("Quit", &().to_variant());
            })
        },
    };
    let _tray = match tray::Tray::start(&connection, tray_actions) {
        Ok(tray) => Some(tray),
        Err(error) => {
            eprintln!(
                "quicklookd: 托盘图标不可用：{error:#}；图标目录 {}",
                tray::icon_directory().display()
            );
            None
        }
    };
    let mut _nemo = None;
    {
        // 必须先注册对象再 RequestName：Nemo 的第一次空格会同时触发服务激活，
        // 它排队的 ShowFile 可能在拿到名字的瞬间就被派发。若此时对象还没注册，
        // 这次调用会返回 UnknownMethod，表现为「第一次按空格没反应」。
        let node = gio::DBusNodeInfo::for_xml(NEMO_XML)?;
        let interface = node.lookup_interface(NEMO_BUS).context("无效 Nemo 协议")?;
        let controller = state.clone();
        let registration = connection
            .register_object(NEMO_PATH, &interface)
            .method_call(move |_, _, _, _, method, parameters, invocation| {
                let result = if method == "Close" {
                    controller.handle("Close", &().to_variant())
                } else if let Some((uri, _, toggle)) = parameters.get::<(String, i32, bool)>() {
                    controller.handle(
                        if toggle { "Toggle" } else { "Show" },
                        &(vec![uri],).to_variant(),
                    )
                } else {
                    Err(anyhow::anyhow!("无效 Nemo 参数"))
                };
                match result {
                    Ok(_) => invocation.return_value(None),
                    Err(error) => invocation
                        .return_dbus_error("org.quicklook.Error.Failed", &error.to_string()),
                }
            })
            .build()?;
        _nemo = Some(registration);
        if replace && nemo {
            state.nemo_owned.set(request_name_retry(
                &connection,
                NEMO_BUS,
                Duration::from_secs(10),
            )?);
        } else {
            state.update_nemo(nemo)?;
        }
    }

    let tray_for_idle = _tray.clone();
    let weak = Rc::downgrade(&state);
    glib::timeout_add_seconds_local(2, move || {
        let Some(state) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let exited = state
            .child
            .borrow_mut()
            .as_mut()
            .map(|c| c.try_wait().ok().flatten().is_some())
            .unwrap_or(false);
        if exited {
            state.child.borrow_mut().take();
        }
        // 托盘图标只存在于这个进程里：宿主收下条目后就必须常驻，否则空闲
        // 超时一到图标连同右键菜单一起消失。没有托盘宿主时才允许空闲退出。
        let tray_holds_broker = tray_for_idle
            .as_ref()
            .is_some_and(|tray| tray.is_registered());
        if !tray_holds_broker
            && state.config.borrow().idle_timeout_secs > 0
            && state.last_action.borrow().elapsed().as_secs()
                > state.config.borrow().idle_timeout_secs
            && state.viewer("Status", None).is_err()
        {
            state.loop_.quit();
        }
        glib::ControlFlow::Continue
    });
    eprintln!("quicklookd ready; lightweight GIO broker");
    loop_.run();
    let _ = state.viewer("Quit", None);
    connection.flush_sync(None::<&gio::Cancellable>)?;
    Ok(())
}
