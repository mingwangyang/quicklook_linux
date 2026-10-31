mod backend;
mod canvas;
mod csv;
mod helper;
mod media;
mod mermaid;
mod native;
mod office;
mod plugins;
mod psd;
mod rtf;
mod ui;
mod worker;
mod x11;

use anyhow::{Context, Result, ensure};
use gio::prelude::*;
use glib::variant::ToVariant;
use quicklook_core::{config::Config, protocol::*};

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
    let (code,) = result.get::<(u32,)>().context("无效总线名称响应")?;
    Ok(code == 1 || code == 4)
}
fn main() {
    if let Err(error) = run() {
        eprintln!("quicklook-viewer: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help") {
        println!(
            "quicklook-viewer [--idle-exit-ms=N] [--detached [--initial-uri=URI]...]\n原生 GTK4 / Wayland 预览窗口；常驻窗口由 quicklookd 管理，分离窗口独立存活"
        );
        return Ok(());
    }
    let mut hidden_exit_ms = None;
    let mut detached = false;
    let mut initial: Vec<String> = Vec::new();
    for arg in &args {
        if let Some(value) = arg.strip_prefix("--idle-exit-ms=") {
            hidden_exit_ms = Some(value.parse::<u64>()?.min(30_000));
        } else if let Some(value) = arg.strip_prefix("--initial-uri=") {
            initial.push(value.to_string());
        } else if arg == "--detached" {
            detached = true;
        } else {
            ensure!(arg == "--exit-on-close", "未知命令行参数");
            hidden_exit_ms = Some(0);
        }
    }
    // 分离窗口不参与常驻生命周期：关闭即退出本进程，不影响主预览。
    if detached {
        hidden_exit_ms = Some(0);
    }
    let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    gtk4::init()?;
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    }
    // 分离窗口使用独立 app-id，避开 niri 的浮动/不抢焦点规则，让它能正常平铺。
    let app_id = if detached { DETACHED_APP_ID } else { APP_ID };
    let app = gtk4::Application::new(Some(app_id), gio::ApplicationFlags::NON_UNIQUE);
    app.register(None::<&gio::Cancellable>)?;
    let _hold = app.hold();
    let mainloop = glib::MainLoop::new(None, false);
    let config = Config::load()?;
    let controller = ui::Controller::new(&app, config, mainloop.clone());
    controller.set_hidden_exit_ms(hidden_exit_ms);
    if detached {
        controller.set_detached();
    }
    if detached && !initial.is_empty() {
        let state = controller.clone();
        glib::idle_add_local_once(move || {
            if let Err(error) = state.show(initial, ui::ShowMode::Show) {
                state.report(&format!("{error:#}"));
            }
        });
    }
    if detached {
        mainloop.run();
        controller.close();
        return Ok(());
    }
    let node = gio::DBusNodeInfo::for_xml(&XML.replace(BUS, VIEWER_BUS))?;
    let interface = node
        .lookup_interface(VIEWER_BUS)
        .context("无效 DBus 协议")?;
    let state = controller.clone();
    let _registration = connection
        .register_object(VIEWER_PATH, &interface)
        .method_call(move |_, _, _, _, method, parameters, invocation| {
            match state.handle(method, &parameters) {
                Ok(value) => invocation.return_value(value.as_ref()),
                Err(error) => invocation
                    .return_dbus_error("org.quicklook.Error.InvalidRequest", &format!("{error:#}")),
            }
        })
        .build()?;
    if !request_name(&connection, VIEWER_BUS)? {
        return Ok(());
    }
    eprintln!("quicklook-viewer ready; backend={}", controller.backend());
    mainloop.run();
    controller.close();
    connection.flush_sync(None::<&gio::Cancellable>)?;
    Ok(())
}
