use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Parser};
use glib::variant::ToVariant;
use quicklook_core::{files::FileInfo, protocol::*};
use std::{
    ffi::OsString,
    io::Read,
    os::unix::ffi::OsStringExt,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Parser, Debug)]
#[command(version=quicklook_core::VERSION,about="原生 Wayland 文件预览；不传路径时打开选文件窗口",group(ArgGroup::new("action").args(["show","next","prev","close","status","quit","pin","unpin","page","extract","screenshot","pick","set_selection","back"])))]
struct Args {
    #[arg(value_name = "PATH", trailing_var_arg = true)]
    paths: Vec<OsString>,
    #[arg(long, help = "显示而不是切换关闭")]
    show: bool,
    #[arg(long, help = "打开独立选文件窗口，不依赖文件管理器")]
    pick: bool,
    #[arg(long, help = "只更新已打开的预览；关闭时不会重新弹出")]
    set_selection: bool,
    #[arg(long, help = "读取 Nautilus 脚本提供的所选文件 URI")]
    from_nautilus: bool,
    #[arg(long)]
    next: bool,
    #[arg(long)]
    prev: bool,
    #[arg(long)]
    close: bool,
    #[arg(long, help = "从包内文件退回上一层（压缩包列表）")]
    back: bool,
    #[arg(long)]
    status: bool,
    #[arg(long)]
    quit: bool,
    #[arg(long)]
    pin: bool,
    #[arg(long)]
    unpin: bool,
    #[arg(long, help = "从标准输入读取 NUL 分隔文件名")]
    stdin0: bool,
    #[arg(long, value_name = "NUMBER", help = "跳转 PDF 页码（从 1 开始）")]
    page: Option<i32>,
    #[arg(
        long,
        value_name = "INDEX",
        help = "预览当前压缩包的指定条目（从 0 开始）"
    )]
    extract: Option<u32>,
    #[arg(long, value_name = "PNG", help = "保存当前预览窗口截图（用于验证）")]
    screenshot: Option<std::path::PathBuf>,
    #[arg(long, help = "打开设置窗口：统一预览键与各文件管理器集成")]
    settings: bool,
    #[arg(long, value_parser = ["caja", "thunar", "nemo"], help = "通过已启用的按键适配启动文件管理器")]
    launch_host: Option<String>,
    #[arg(long, help = "按当前配置重新写入各文件管理器集成，不开窗口")]
    apply_hosts: bool,
    #[arg(long, help = "只维护合成器窗口规则与登录自启条目，不动文件管理器集成")]
    apply_window_rules: bool,
    #[arg(long, help = "移除所有由 Quick Look 写入的文件管理器集成")]
    remove_hosts: bool,
    #[arg(
        long,
        help = "检测已安装的文件管理器并自动写入接入、打开接管（首次安装用）"
    )]
    autodetect_hosts: bool,
    #[arg(
        long,
        help = "与 --apply-hosts / --remove-hosts 搭配，只显示会改动的文件"
    )]
    dry_run: bool,
}
fn main() {
    if let Err(error) = run() {
        eprintln!("quicklook: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let mut args = Args::parse();
    if let Some(host) = args.launch_host {
        let launcher = quicklook_core::config::xdg_data_home()
            .join("quicklook/launchers")
            .join(format!("quicklook-{host}"));
        if !launcher.is_file() {
            bail!("{host} 按键适配未启用；请在设置中选择接管并应用接入");
        }
        use std::os::unix::process::CommandExt;
        return Err(Command::new(launcher).args(&args.paths).exec().into());
    }
    if args.settings {
        let exe = std::env::current_exe()?
            .parent()
            .context("无法定位程序目录")?
            .join("quicklook-settings");
        if !exe.is_file() {
            bail!(
                "找不到 {}，请先构建或安装 quicklook-settings",
                exe.display()
            );
        }
        let mut command = Command::new(exe);
        // XWayland 窗口在 niri 里没有 app-id，设置窗口的浮动规则会失效。
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            command.env_remove("GDK_BACKEND");
        }
        let status = command.status().context("无法打开设置窗口")?;
        if !status.success() {
            bail!("设置窗口异常退出：{status}");
        }
        return Ok(());
    }
    if args.apply_window_rules {
        let config = quicklook_core::config::Config::load()?;
        let report = quicklook_core::hosts::apply_window_rules_only(&config, args.dry_run)?;
        if args.dry_run {
            println!("预演（未改动任何文件）：{}", report.summary());
        } else {
            println!("{}", report.summary());
        }
        for path in &report.written {
            println!("写入 {}", path.display());
        }
        for note in &report.notes {
            println!("{note}");
        }
        return Ok(());
    }
    if args.autodetect_hosts {
        let base = quicklook_core::config::Config::load()?;
        let (config, hosts) = quicklook_core::hosts::autodetect(&base);
        if hosts.is_empty() {
            println!(
                "未检测到已知的文件管理器（nemo / nautilus / dolphin / thunar / caja / yazi），未做改动。"
            );
            return Ok(());
        }
        if args.dry_run {
            println!(
                "预演（未改动任何文件）：检测到 {}；将打开接管并写入接入。",
                hosts.join("、")
            );
            return Ok(());
        }
        config.save().context("写入配置失败")?;
        let report = quicklook_core::hosts::apply(&config)?;
        println!("检测到已安装：{}。", hosts.join("、"));
        println!("{}", report.summary());
        for path in &report.written {
            println!("写入 {}", path.display());
        }
        for note in &report.notes {
            println!("{note}");
        }
        println!("已打开预览按键接管；如需撤销请打开「Quick Look 设置」取消勾选。");
        return Ok(());
    }
    if args.apply_hosts || args.remove_hosts {
        let mut config = quicklook_core::config::Config::load()?;
        if args.remove_hosts {
            config.hijack_native = false;
            config.host_nemo = false;
            config.host_dolphin = false;
            config.host_nautilus = false;
            config.host_thunar = false;
            config.host_caja = false;
            config.host_yazi = false;
            config.autostart = false;
        }
        let report = quicklook_core::hosts::apply_with(&config, args.dry_run)?;
        if args.dry_run {
            println!("预演（未改动任何文件）：{}", report.summary());
        } else {
            println!("{}", report.summary());
        }
        for path in &report.written {
            println!("写入 {}", path.display());
        }
        for path in &report.removed {
            println!("移除 {}", path.display());
        }
        for note in &report.notes {
            println!("{note}");
        }
        return Ok(());
    }
    if args.from_nautilus {
        let value = std::env::var("NAUTILUS_SCRIPT_SELECTED_URIS")
            .context("此入口需要 Nautilus 脚本提供所选文件")?;
        if value.len() > 16 * 1024 * 1024 {
            bail!("文件列表超过 16 MiB 限额");
        }
        for uri in value.lines().filter(|s| !s.is_empty()) {
            if !uri.starts_with("file://") {
                bail!("仅支持本机文件 URI");
            }
            args.paths.push(uri.into());
        }
        if args.paths.is_empty() {
            bail!("Nautilus 没有选择本机文件");
        }
    }
    if args.stdin0 {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16 * 1024 * 1024 {
            bail!("文件列表超过 16 MiB 限额");
        }
        args.paths.extend(
            bytes
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|b| OsString::from_vec(b.to_vec())),
        );
    }
    let method = if args.pick {
        "Pick"
    } else if args.set_selection {
        "SetSelection"
    } else if args.next {
        "Next"
    } else if args.prev {
        "Prev"
    } else if args.close {
        "Close"
    } else if args.back {
        "Back"
    } else if args.status {
        "Status"
    } else if args.quit {
        "Quit"
    } else if args.pin || args.unpin {
        "Pin"
    } else if args.page.is_some() {
        "Page"
    } else if args.extract.is_some() {
        "Extract"
    } else if args.screenshot.is_some() {
        "Screenshot"
    } else if args.show {
        if args.paths.is_empty() {
            "Pick"
        } else {
            "Show"
        }
    } else if args.paths.is_empty() && !args.stdin0 {
        "Pick"
    } else {
        "Toggle"
    };
    if args.paths.len() > 4096 {
        bail!("最多选择 4096 个文件");
    }
    let uris: Vec<String> = args
        .paths
        .iter()
        .map(FileInfo::from_arg)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|i| i.uri)
        .collect();
    if matches!(method, "Show" | "Toggle") && uris.is_empty() {
        bail!("请提供文件路径，或使用 --stdin0 / --help");
    }
    if !matches!(method, "Show" | "Toggle" | "SetSelection") && !uris.is_empty() {
        bail!("{method} 不接受文件路径");
    }
    let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    let params = if matches!(method, "Show" | "Toggle" | "SetSelection") {
        Some((uris,).to_variant())
    } else if method == "Pin" {
        Some((args.pin,).to_variant())
    } else if let Some(page) = args.page {
        Some((page,).to_variant())
    } else if let Some(index) = args.extract {
        Some((index,).to_variant())
    } else if let Some(path) = &args.screenshot {
        let path = if path.is_absolute() {
            path.clone()
        } else {
            std::env::current_dir()?.join(path)
        };
        Some((path.to_string_lossy().into_owned(),).to_variant())
    } else {
        None
    };
    let passive = matches!(method, "Close" | "Status" | "Quit" | "SetSelection");
    let result = connection.call_sync(
        Some(BUS),
        PATH,
        BUS,
        method,
        params.as_ref(),
        None,
        if passive {
            gio::DBusCallFlags::NO_AUTO_START
        } else {
            gio::DBusCallFlags::NONE
        },
        3000,
        None::<&gio::Cancellable>,
    );
    let reply = match result {
        Ok(r) => r,
        Err(e)
            if matches!(
                e.kind::<gio::DBusError>(),
                Some(gio::DBusError::ServiceUnknown | gio::DBusError::NameHasNoOwner)
            ) =>
        {
            if passive {
                if args.status {
                    println!("{{\"running\":false,\"visible\":false}}");
                }
                return Ok(());
            }
            let executable = std::env::current_exe()?
                .parent()
                .context("无法定位程序目录")?
                .join("quicklookd");
            if !executable.is_file() {
                bail!("找不到 {}，请先构建或安装 quicklookd", executable.display());
            }
            let log_dir = quicklook_core::config::cache_dir();
            std::fs::create_dir_all(&log_dir)?;
            let log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("daemon.log"))?;
            Command::new(executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(log))
                .spawn()?;
            let mut response = None;
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(50));
                match connection.call_sync(
                    Some(BUS),
                    PATH,
                    BUS,
                    method,
                    params.as_ref(),
                    None,
                    gio::DBusCallFlags::NO_AUTO_START,
                    1000,
                    None::<&gio::Cancellable>,
                ) {
                    Ok(r) => {
                        response = Some(r);
                        break;
                    }
                    Err(e)
                        if matches!(
                            e.kind::<gio::DBusError>(),
                            Some(gio::DBusError::ServiceUnknown | gio::DBusError::NameHasNoOwner)
                        ) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            response.context("预览服务启动超时；请查看 ~/.cache/quicklook/daemon.log")?
        }
        Err(e) => return Err(e.into()),
    };
    if args.status {
        let (value,) = reply.get::<(String,)>().context("无效状态响应")?;
        println!("{value}");
    }
    Ok(())
}
