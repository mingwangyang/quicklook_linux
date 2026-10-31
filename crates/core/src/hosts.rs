//! 把设置窗口里的选择落到各文件管理器的用户级集成文件。
//!
//! 只改动自己写过的托管标记块与自己的文件，用户原有配置一律保留。行为与
//! 安装器、设置窗口与命令行共用同一套实现；不依赖源码目录。
use crate::{
    config::{Config, xdg_config_home, xdg_data_home},
    keys::PreviewKey,
};
use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};

/// The selection vfunc layout was verified for this exact x86_64 Nemo build.
/// Reinstalling must not turn an unknown binary into an ABI-compatible one.
pub const VERIFIED_NEMO_FINGERPRINT: u64 = 0xd1b0_31c7_a1d6_0e84;

const BEGIN: &str = "# BEGIN quicklook managed integration";
const END: &str = "# END quicklook managed integration";
const BEGIN_LUA: &str = "-- BEGIN quicklook managed integration";
const END_LUA: &str = "-- END quicklook managed integration";
const BEGIN_XML: &str = "<!-- BEGIN quicklook managed integration -->";
const END_XML: &str = "<!-- END quicklook managed integration -->";
/// Nautilus 脚本菜单项名称，同时是 scripts-accels 里我们那几行的标识。
const SCRIPT_ITEM: &str = "Quick Look";
const DOLPHIN: &str = include_str!("../../../integration/dolphin/quicklook.desktop.in");
const YAZI: &str = include_str!("../../../integration/yazi/quicklook.yazi/main.lua");
const THUNAR: &str = include_str!("../../../integration/thunar/quicklook.uca.xml.in");
const CAJA_MENU: &str = include_str!("../../../integration/caja/quicklook.py.in");
const THUNAR_BUSES: &[&str] = &["org.xfce.Thunar", "org.xfce.FileManager"];
/// Caja 启动时可能用到的条目：`caja-browser` 是面板/菜单里那个可见项，
/// `caja-folder-handler` 是别的应用「打开文件夹」时真正走的条目，其余是快捷入口。
/// 只换 Exec 的第一个命令，其它字段原样保留。
const CAJA_ENTRIES: &[&str] = &[
    "caja.desktop",
    "caja-browser.desktop",
    "caja-folder-handler.desktop",
    "caja-home.desktop",
    "caja-computer.desktop",
];
/// 应用图标。所有尺寸都嵌进二进制，这样只装可执行文件也能取到图标，
/// 不必假设 <prefix>/share 里还留着源码树的 integration 目录。
const ICONS: &[(&str, &[u8])] = &[
    (
        "scalable/apps/org.quicklook.Preview.svg",
        include_bytes!("../../../integration/icons/org.quicklook.Preview.svg"),
    ),
    (
        "16x16/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/16x16/apps/org.quicklook.Preview.png"),
    ),
    (
        "22x22/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/22x22/apps/org.quicklook.Preview.png"),
    ),
    (
        "24x24/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/24x24/apps/org.quicklook.Preview.png"),
    ),
    (
        "32x32/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/32x32/apps/org.quicklook.Preview.png"),
    ),
    (
        "48x48/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/48x48/apps/org.quicklook.Preview.png"),
    ),
    (
        "64x64/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/64x64/apps/org.quicklook.Preview.png"),
    ),
    (
        "128x128/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/128x128/apps/org.quicklook.Preview.png"),
    ),
    (
        "256x256/apps/org.quicklook.Preview.png",
        include_bytes!("../../../integration/icons/hicolor/256x256/apps/org.quicklook.Preview.png"),
    ),
];

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub written: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub notes: Vec<String>,
    /// 预演模式：仍然算出会写/会删哪些文件，但不碰磁盘。
    dry_run: bool,
}
impl Report {
    /// 给设置界面显示的一行摘要。
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.written.is_empty() {
            parts.push(format!("写入 {} 个文件", self.written.len()));
        }
        if !self.removed.is_empty() {
            parts.push(format!("移除 {} 个文件", self.removed.len()));
        }
        if parts.is_empty() {
            parts.push("已是最新，无需改动".to_string());
        }
        parts.join("，")
    }
}

/// 用当前安装位置和 XDG 目录应用设置。
/// 在 PATH 里找可执行文件（不依赖 `which`，避免最小化环境里没有）。
fn command_available(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(program);
                std::fs::metadata(&candidate)
                    .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// 检测本机实际安装的文件管理器 / 终端宿主。
///
/// 返回内部主机键（`nemo` / `nautilus` / `dolphin` / `thunar` / `caja` / `yazi`），
/// 顺序固定，方便日志与设置窗口展示。只依据可执行文件是否存在，不去猜用户偏好。
pub fn detect_hosts() -> Vec<&'static str> {
    [
        ("nemo", "nemo"),
        ("nautilus", "nautilus"),
        ("dolphin", "dolphin"),
        ("thunar", "thunar"),
        ("caja", "caja"),
        ("yazi", "yazi"),
    ]
    .into_iter()
    .filter(|(_, program)| command_available(program))
    .map(|(key, _)| key)
    .collect()
}

/// 首次安装用：把检测到的宿主写进配置并打开接管，返回 (配置, 检测到的宿主)。
///
/// 用户已有的选择不会被覆盖——调用方应先判断「配置里是否已经明确选择过接管」。
pub fn autodetect(base: &Config) -> (Config, Vec<&'static str>) {
    let hosts = detect_hosts();
    let mut config = base.clone();
    config.host_nemo = hosts.contains(&"nemo");
    config.host_nautilus = hosts.contains(&"nautilus");
    config.host_dolphin = hosts.contains(&"dolphin");
    config.host_thunar = hosts.contains(&"thunar");
    config.host_caja = hosts.contains(&"caja");
    config.host_yazi = hosts.contains(&"yazi");
    config.hijack_native = !hosts.is_empty();
    (config, hosts)
}

pub fn apply(config: &Config) -> Result<Report> {
    apply_with(config, false)
}

/// 预演时不写任何文件，只返回改动清单。
pub fn apply_with(config: &Config, dry_run: bool) -> Result<Report> {
    apply_at_with(
        config,
        &install_prefix()?,
        &xdg_data_home(),
        &xdg_config_home(),
        dry_run,
    )
}

/// 可注入目录的版本，供测试与打包脚本使用。
pub fn apply_at(config: &Config, prefix: &Path, data: &Path, config_home: &Path) -> Result<Report> {
    apply_at_with(config, prefix, data, config_home, false)
}

pub fn apply_at_with(
    config: &Config,
    prefix: &Path,
    data: &Path,
    config_home: &Path,
    dry_run: bool,
) -> Result<Report> {
    apply_at_impl(config, prefix, data, config_home, dry_run, None, None)
}

fn apply_at_impl(
    config: &Config,
    prefix: &Path,
    data: &Path,
    config_home: &Path,
    dry_run: bool,
    nemo_bin: Option<&Path>,
    nemo_fp: Option<u64>,
) -> Result<Report> {
    let mut report = Report {
        dry_run,
        ..Report::default()
    };
    for shared in ["Thunar/uca.xml", "yazi/init.lua", "yazi/keymap.toml"] {
        let path = config_home.join(shared);
        strip_managed(&read_text(&path)?).with_context(|| format!("检查 {}", path.display()))?;
    }
    let cli = cli_path(prefix);
    // Nemo：右键动作。带 LD_PRELOAD 的启动器由安装器生成，设置只负责菜单项。
    let nemo = data.join("nemo/actions/quicklook.nemo_action");
    if config.host_nemo {
        write(
            &nemo,
            &format!(
                "[Nemo Action]\nName=用 Quick Look 预览\nComment=预览选中的文件\nExec=\"{}\" --show %U\nIcon-Name=org.quicklook.Preview\nSelection=notnone\nExtensions=any;\nQuote=double\n",
                desktop_exec_arg(&cli)
            ),
            false,
            &mut report,
        )?;
    } else {
        remove(&nemo, &mut report)?;
    }
    apply_nemo_at(config, prefix, data, &mut report, nemo_bin, nemo_fp)?;
    // niri 窗口规则决定预览是浮动还是被平铺，deb 安装不碰用户配置，只能由这里维护。
    let any_host = config.host_nemo
        || config.host_dolphin
        || config.host_nautilus
        || config.host_thunar
        || config.host_caja
        || config.host_yazi;
    apply_window_rules(config, config_home, &cli, any_host, &mut report);
    // 登录自启独立于文件管理器接入：它只是我们自己的一个 XDG autostart 条目。
    apply_autostart(config, config_home, prefix, &mut report)?;
    // Dolphin: service menu; 26.04 + KIO 6.24 can bind this action in Configure Shortcuts.
    let dolphin = data.join("kio/servicemenus/quicklook.desktop");
    let dolphin_before = std::fs::read_to_string(&dolphin).ok();
    if config.host_dolphin {
        write(
            &dolphin,
            &DOLPHIN.replace("@QUICKLOOK_CLI@", &desktop_exec_arg(&cli)),
            true,
            &mut report,
        )?;
    } else {
        remove(&dolphin, &mut report)?;
    }
    // 内容没变也要刷新：缓存可能停在更早的某次安装上，不重建就会一直缺菜单。
    if !dry_run && (config.host_dolphin || dolphin_before.is_some()) {
        refresh_kde_service_cache(data);
    }
    if config.host_dolphin && config.hijack_native {
        report.notes.push("Dolphin 23.08 仅右键；26.04+ 且 KIO 6.24+ 可在 Dolphin「配置快捷键 → 上下文菜单动作」为 Quick Look 指定空格（需手动绑定）。".into());
    }
    // Nautilus：脚本菜单负责完整多选。
    let script = data.join("nautilus/scripts/Quick Look");
    if config.host_nautilus {
        write(
            &script,
            &format!("#!/bin/sh\nexec {} --from-nautilus\n", shell_quote(&cli)),
            true,
            &mut report,
        )?;
    } else {
        remove(&script, &mut report)?;
    }
    // Nautilus 原生空格预览：只有接管时才占用宿主自己的预览入口。
    let service = data.join("dbus-1/services/org.gnome.NautilusPreviewer.service");
    if config.host_nautilus && config.hijack_native {
        write_desktop_override(
            &service,
            "quicklook",
            &format!(
                "[D-BUS Service]\nName=org.gnome.NautilusPreviewer\nExec={}\n",
                json_path(&bridge_path(prefix))
            ),
            &mut report,
        )?;
    } else {
        remove_desktop_override(&service, "quicklook", &mut report)?;
    }
    apply_thunar(config, &cli, config_home, &mut report)?;
    apply_thunar_adapter(config, prefix, data, config_home, &mut report)?;
    apply_launcher_shim(config, prefix, data, config_home, &mut report)?;
    apply_caja(config, prefix, data, &mut report)?;
    apply_icons(data, &report)?;
    apply_accels(config, &config.nautilus_key(), config_home, &mut report)?;
    apply_yazi(config, &config.yazi_key(), &cli, config_home, &mut report)?;
    if config.host_nautilus && config.hijack_native && !config.nautilus_key().nautilus_bindable() {
        report
            .notes
            .push("Nautilus 脚本加速键只接受 Ctrl 或 Alt，空格由宿主原生预览接管。".into());
    }
    if !dry_run && data == xdg_data_home() && config_home == xdg_config_home() {
        reload_running_service(config);
    }
    Ok(report)
}

/// Changes to takeover settings must affect a resident broker without closing previews.
fn reload_running_service(config: &Config) {
    use glib::variant::ToVariant;
    if let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) {
        let _ = bus.call_sync(
            Some(crate::protocol::BUS),
            crate::protocol::PATH,
            crate::protocol::BUS,
            "ReloadConfig",
            Some(&(config.host_nemo && config.hijack_native,).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            1500,
            None::<&gio::Cancellable>,
        );
    }
}

/// Thunar 的自定义动作是 XML 文件，不能像 shell 脚本那样行式追加：
/// 必须把 `<action>` 塞进已有的 `<actions>` 根里，否则 Thunar 会整个文件解析失败，
/// 连用户自己的动作一起丢掉。
fn apply_thunar(
    config: &Config,
    cli: &Path,
    config_home: &Path,
    report: &mut Report,
) -> Result<()> {
    let path = config_home.join("Thunar/uca.xml");
    let old = read_text(&path)?;
    let clean = strip_managed(&old)?;
    if !config.host_thunar {
        if !old.contains(BEGIN_XML) {
            return Ok(());
        }
        // 没有别的动作就说明这个文件当初只是为我们建的，关掉时一起删掉；
        // 留着空的 `<actions>` 反而让 Thunar 弹出一个空的自定义动作菜单。
        if clean.trim().is_empty()
            || clean.trim() == "<actions>\n</actions>"
            || clean.trim() == "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n</actions>"
        {
            return remove(&path, report);
        }
        if clean.trim() != old.trim() {
            return write(&path, &(clean.trim_end().to_string() + "\n"), false, report);
        }
        return Ok(());
    }
    let action = THUNAR.replace(
        "@QUICKLOOK_CLI@",
        &glib::markup_escape_text(&shell_quote(cli)),
    );
    let block = format!("{BEGIN_XML}\n{}\n{END_XML}", action.trim_end());
    let merged = if clean.trim().is_empty() {
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n{block}\n</actions>\n")
    } else if let Some(at) = clean.rfind("</actions>") {
        let head = clean[..at].trim_end();
        let tail = clean[at..].trim_start();
        format!("{head}\n{block}\n{tail}\n")
    } else {
        report.notes.push(
            "Thunar 的自定义动作文件缺少 </actions>，未改动；请手工检查 ~/.config/Thunar/uca.xml"
                .into(),
        );
        return Ok(());
    };
    write(&path, &merged, false, report)
}

/// Nemo 的空格默认由宿主自己绑到 `org.nemo.Preview.ShowFile`，我们只需要提供
/// 同名 D-Bus 服务。但一旦用户把预览键改成别的组合，宿主的裸空格就必须被拦住，
/// 否则配置看起来完全不生效；这时改走适配器，在 Nemo 的视图处理按键之前拦截。
/// 维护合成器窗口规则：平铺合成器不给规则的话，预览会被当成普通窗口塞进布局。
///
/// `enable` 为假表示用户把所有宿主集成都关掉了，连规则一起撤掉。
fn apply_window_rules(
    config: &Config,
    config_home: &Path,
    cli: &Path,
    enable: bool,
    report: &mut Report,
) {
    if enable {
        match crate::niri::install_at(
            config_home,
            config.uses_floating_layout(),
            cli,
            report.dry_run,
        ) {
            Ok(Some((path, changed))) => {
                if changed {
                    report.written.push(path);
                }
            }
            Ok(None) => {
                if let Some(hint) = crate::compositor::hint(config_home) {
                    report.notes.push(hint);
                }
            }
            Err(error) => report.notes.push(format!("窗口规则未更新：{error:#}")),
        }
    } else if let Ok(Some(path)) = crate::niri::remove_at(config_home, report.dry_run) {
        report.removed.push(path);
    }
}

/// 只维护合成器窗口规则，不碰文件管理器接入；安装脚本在用户尚未选择接管时用它。
pub fn apply_window_rules_only(config: &Config, dry_run: bool) -> Result<Report> {
    let mut report = Report {
        dry_run,
        ..Report::default()
    };
    let prefix = install_prefix()?;
    let config_home = xdg_config_home();
    let cli = cli_path(&prefix);
    apply_window_rules(config, &config_home, &cli, true, &mut report);
    // 自启条目和窗口规则一样属于「装完就该对」的东西，跟着这个窄入口一起对齐，
    // 安装器与维护者脚本就不用各自再写一遍 desktop 文件。
    apply_autostart(config, &config_home, &prefix, &mut report)?;
    Ok(report)
}

/// 测试钩子：由调用方显式传入伪 nemo 二进制路径替代 /usr/bin/nemo 做 ABI 指纹验证，
/// 让「改键装旁路器」的链路可以在没有真实 Nemo 的机器（CI）上端到端验证。
/// 用参数而不是环境变量：不引入 unsafe，也不受测试并行影响。
#[cfg(test)]
pub(crate) fn apply_for_test(
    config: &Config,
    prefix: &Path,
    data: &Path,
    config_home: &Path,
    nemo_bin: Option<&Path>,
    nemo_fp: u64,
) -> Result<Report> {
    apply_at_impl(
        config,
        prefix,
        data,
        config_home,
        false,
        nemo_bin,
        Some(nemo_fp),
    )
}

fn apply_nemo_at(
    config: &Config,
    prefix: &Path,
    data: &Path,
    report: &mut Report,
    binary: Option<&Path>,
    nemo_fp: Option<u64>,
) -> Result<()> {
    let library = prefix.join("lib/quicklook/libquicklook_nemo.so");
    let launcher = launcher_path(data, "nemo");
    let entry = data.join("applications/nemo.desktop");
    // 早期版本还生成过独立的应用菜单项，同样指向 quicklook-nemo；留着会在菜单里
    // 出现一个点了没反应的「文件（Quick Look）」。
    let legacy_entry = data.join("applications/org.quicklook.Nemo.desktop");
    let service = data.join("dbus-1/services/org.nemo.Preview.service");
    remove_owned(&legacy_entry, "quicklook-nemo", report)?;
    if config.host_nemo && config.hijack_native {
        write_desktop_override(
            &service,
            "quicklook",
            &format!(
                "[D-BUS Service]\nName=org.nemo.Preview\nExec={} --nemo-compat\n",
                json_path(&broker_path(prefix))
            ),
            report,
        )?;
    } else {
        remove_desktop_override(&service, "quicklook", report)?;
    }

    let remap =
        config.host_nemo && config.hijack_native && config.nemo_key() != PreviewKey::default();
    let expected = nemo_fp.unwrap_or(VERIFIED_NEMO_FINGERPRINT);
    let pin = std::fs::read(binary.unwrap_or(Path::new("/usr/bin/nemo")))
        .ok()
        .filter(|bytes| cfg!(target_arch = "x86_64") && fnv1a(bytes) == expected)
        .map(|_| format!(" QUICKLOOK_NEMO_FINGERPRINT={expected:016x}"));
    let Some(pin) = pin.filter(|_| library.is_file()) else {
        remove(&launcher, report)?;
        remove_desktop_override(&entry, "quicklook-nemo", report)?;
        if remap {
            report.notes.push(
                "Nemo 快捷键接管未启用：缺少适配器，或宿主构建未通过 ABI 验证；默认空格与右键仍可用"
                    .into(),
            );
        }
        return Ok(());
    };
    if !remap {
        remove(&launcher, report)?;
        remove_desktop_override(&entry, "quicklook-nemo", report)?;
        return Ok(());
    }
    let script = launcher_script(&library, "nemo", "org.Nemo", "Nemo", &pin);
    write(&launcher, &script, true, report)?;
    let desktop = desktop_with_launcher(&system_desktop("nemo.desktop"), "nemo", &launcher)
        .unwrap_or_else(|| minimal_desktop("Files", &launcher));
    write_desktop_override(&entry, "quicklook-nemo", &desktop, report)?;
    Ok(())
}

/// 生成文件管理器的 LD_PRELOAD 启动器脚本。
///
/// 关键行为：宿主已经在跑时先判断它是哪种状态——
/// - `adapted`：加载的就是当前这份库，直接交给它；
/// - `stale`：加载的是旧版库，功能仍在，不打断用户；
/// - `foreign`：根本没加载适配器（从终端、旧版自启项或别的入口起来的），
///   这时新进程只会把旧窗口抬到前面，只能静默重启一次，不再弹确认框。
fn launcher_script(lib: &Path, program: &str, bus: &str, label: &str, extra_env: &str) -> String {
    let lib = shell_quote(lib);
    format!(
        r#"#!/usr/bin/env bash
# 由 quicklook --apply-hosts 生成：让 {program} 在带适配器的进程里启动。
set -uo pipefail
lib={lib}
running=$(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.GetConnectionUnixProcessID {bus} 2>/dev/null | sed -n 's/.*uint32 \([0-9]*\).*/\1/p')
current_inode=$(stat -Lc %i -- "$lib" 2>/dev/null || true)
state=foreign
if [[ -n $running && -n $current_inode ]]; then
    while read -r _addr _perms _offset _dev inode rest; do
        # 覆盖安装后旧映射会显示成 "…/libquicklook_thunar.so (deleted)"，所以按前缀比。
        if [[ $rest == "$lib"* ]]; then
            if [[ $inode == $current_inode ]]; then state=adapted; else state=stale; fi
            break
        fi
    done < <(grep -F -- "$lib" "/proc/$running/maps" 2>/dev/null || true)
fi
log=${{XDG_CACHE_HOME:-$HOME/.cache}}/quicklook/launcher.log
note() {{ mkdir -p "$(dirname "$log")" 2>/dev/null || true; printf '%s {program}: %s\n' "$(date '+%F %T')" "$1" >>"$log" 2>/dev/null || true; }}
if [[ $state == stale ]]; then
    note "运行中的实例用的是旧版适配器，未重启（pid ${{running:-?}}）"
elif [[ $state == foreign && -n ${{running:-}} ]]; then
    if [[ ${{QUICKLOOK_LAUNCHER_ASK:-0}} == 1 ]] && command -v zenity >/dev/null; then
        zenity --question --title='Quick Look' --text='{label} 正在运行，但没有加载空格预览适配。需要重启 {label} 才能启用，现在重启吗？' --ok-label='重启 {label}' --cancel-label='取消' 2>/dev/null || {{ note "用户取消重启"; exit 0; }}
    fi
    note "运行中的实例未加载适配器，自动重启（pid ${{running:-?}}）"
    /usr/bin/{program} --quit >/dev/null 2>&1 || true
    if [[ -n ${{running:-}} ]]; then
        # 用会话总线上的所有者 pid 判断退出，别用 pgrep：pgrep 是全局的，会看到
        # 别的会话/终端里的同名进程，永远等不到它消失。
        owner_pid() {{ gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.GetConnectionUnixProcessID {bus} 2>/dev/null | sed -n 's/.*uint32 \([0-9]*\).*/\1/p'; }}
        for _ in {{1..30}}; do
            now=$(owner_pid)
            [[ -z ${{now:-}} || $now != "$running" ]] && break
            sleep 0.1
        done
        now=$(owner_pid)
        if [[ -n ${{now:-}} && $now == "$running" ]]; then
            note "宿主没有响应 --quit，改为发送终止信号（pid $running）"
            kill -TERM "$running" >/dev/null 2>&1 || true
            sleep 0.5
            kill -KILL "$running" >/dev/null 2>&1 || true
            sleep 0.3
        fi
    fi
fi
exec env{extra_env} LD_PRELOAD="$lib${{LD_PRELOAD:+:$LD_PRELOAD}}" /usr/bin/{program} "$@"
"#
    )
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

/// 用户选择启用接管之后生成的启动器。软件包的 /usr 前缀必须保持只读。
fn launcher_path(data: &Path, program: &str) -> PathBuf {
    data.join("quicklook/launchers")
        .join(format!("quicklook-{program}"))
}

/// 撤销入口时只删指向自己启动器的那个链接，用户自己的文件一律不碰。
fn remove_path_shim(program: &str, data: &Path, report: &mut Report) -> Result<()> {
    let Some(dir) = data.parent().map(|share| share.join("bin")) else {
        return Ok(());
    };
    let shim = dir.join(program);
    let Ok(metadata) = shim.symlink_metadata() else {
        return Ok(());
    };
    if metadata.file_type().is_symlink()
        && std::fs::read_link(&shim).ok().is_some_and(|target| {
            target
                .to_string_lossy()
                .contains(&format!("quicklook-{program}"))
        })
    {
        remove(&shim, report)?;
    }
    Ok(())
}

/// 系统 desktop 条目的位置。只在 `/usr/share/applications` 里找：这两种文件管理器
/// 都由发行版装在那里，找不到就走最小条目兜底。
fn system_desktop(name: &str) -> PathBuf {
    PathBuf::from("/usr/share/applications").join(name)
}

fn minimal_desktop(title: &str, launcher: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={title}\nComment=文件管理器（空格用 Quick Look 预览）\nExec=\"{}\" %U\nIcon=system-file-manager\nTerminal=false\nCategories=System;FileTools;FileManager;\nMimeType=inode/directory;\n",
        desktop_exec_arg(launcher)
    )
}

/// 把系统条目里 `Exec=<program> ...` 的首个命令换成我们的启动器，其余原样保留。
///
/// 直接重写整份条目会丢掉发行版附带的 `Actions`（主目录 / 计算机 / 回收站）与多语言
/// 名字，所以这里只动 Exec 行；一条也没换到就返回 None，让调用方退回最小条目。
fn desktop_with_launcher(system: &Path, program: &str, launcher: &Path) -> Option<String> {
    let text = std::fs::read_to_string(system).ok()?;
    let quoted = format!("\"{}\"", desktop_exec_arg(launcher));
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Exec=") {
            let mut parts = rest.splitn(2, ' ');
            let command = parts.next().unwrap_or("");
            let tail = parts.next().unwrap_or("");
            let base = Path::new(command)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(command);
            if base == program {
                out.push_str("Exec=");
                out.push_str(&quoted);
                if !tail.is_empty() {
                    out.push(' ');
                    out.push_str(tail);
                }
                out.push('\n');
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    replaced.then_some(out)
}

/// niri-stack 的 Mod+E 脚本直接 exec `thunar`，绕过 desktop 覆盖，适配器就进不了
/// 进程。deb 安装不碰家目录，过去只有源码安装器会改写它，于是从源码版迁到 deb 后
/// 空格失效。这里把同一套「只改已知命令、带受管标记、可撤销」的迁移搬进 apply。
const SHIM_BEGIN: &str = "# BEGIN quicklook managed file-manager launcher\n";
const SHIM_END: &str = "# END quicklook managed file-manager launcher\n";
const SHIM_COMMAND: &str = "exec env -u GTK_IM_MODULE GDK_BACKEND=wayland thunar \"$@\"";

/// 去掉受管块；标记不成对说明有人手工改过，报错而不是猜。
fn shim_strip(text: &str) -> Result<String> {
    let begin = text.matches(SHIM_BEGIN).count();
    let end = text.matches(SHIM_END).count();
    if begin == 0 && end == 0 {
        return Ok(text.to_string());
    }
    ensure!(
        begin == 1 && end == 1,
        "文件管理器启动脚本的 Quick Look 标记不完整，未改动"
    );
    let start = text.find(SHIM_BEGIN).expect("已确认存在");
    let finish = text.find(SHIM_END).expect("已确认存在");
    ensure!(
        finish > start,
        "文件管理器启动脚本的 Quick Look 标记顺序错误，未改动"
    );
    Ok(format!(
        "{}{}",
        &text[..start],
        &text[finish + SHIM_END.len()..]
    ))
}

/// 只在脚本里恰好有一行那条已知命令时改写；否则返回 None 保留原样。
fn shim_integrate(text: &str, launcher: &Path) -> Result<Option<String>> {
    let original = shim_strip(text)?;
    if original
        .lines()
        .filter(|line| *line == SHIM_COMMAND)
        .count()
        != 1
    {
        return Ok(None);
    }
    let quoted = shell_quote(launcher);
    let block = format!(
        "{SHIM_BEGIN}if [ -x {quoted} ]; then\n    exec env -u GTK_IM_MODULE GDK_BACKEND=wayland {quoted} \"$@\"\nfi\n{SHIM_END}"
    );
    Ok(Some(original.replacen(
        SHIM_COMMAND,
        &format!("{block}{SHIM_COMMAND}"),
        1,
    )))
}

/// 可能的宿主启动脚本位置：源码安装的 prefix，以及 niri-stack 的固定布局。
fn shim_paths(prefix: &Path, config_home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !paths.contains(&path) {
            paths.push(path);
        }
    };
    push(prefix.join("bin/launch-file-manager"));
    if let Some(home) = config_home.parent() {
        push(home.join(".niri-stack/prefix/bin/launch-file-manager"));
    }
    paths
}

fn apply_launcher_shim(
    config: &Config,
    prefix: &Path,
    data: &Path,
    config_home: &Path,
    report: &mut Report,
) -> Result<()> {
    let launcher = data.join("quicklook/launchers/quicklook-thunar");
    let takeover = config.host_thunar && config.hijack_native && launcher.is_file();
    for path in shim_paths(prefix, config_home) {
        if !path.is_file() || path.is_symlink() {
            continue;
        }
        let text = read_text(&path)?;
        let updated = if takeover {
            match shim_integrate(&text, &launcher)
                .with_context(|| format!("检查 {}", path.display()))?
            {
                Some(updated) => Some(updated),
                None => {
                    report.notes.push(format!(
                        "{} 的启动命令不是已知形式，已保留原样；要让空格生效，请自己把它指向 {}",
                        path.display(),
                        launcher.display()
                    ));
                    None
                }
            }
        } else {
            let stripped = shim_strip(&text)?;
            (stripped != text).then_some(stripped)
        };
        if let Some(updated) = updated {
            write(&path, &updated, true, report)?;
        }
    }
    Ok(())
}

fn apply_thunar_adapter(
    config: &Config,
    prefix: &Path,
    data: &Path,
    config_home: &Path,
    report: &mut Report,
) -> Result<()> {
    let library = prefix.join("lib/quicklook/libquicklook_thunar.so");
    let launcher = launcher_path(data, "thunar");
    let entry = data.join("applications/thunar.desktop");
    let wanted = config.host_thunar && config.hijack_native;
    if !wanted || !library.is_file() {
        apply_thunar_activation(false, &launcher, data, config_home, report)?;
        remove(&launcher, report)?;
        remove_path_shim("thunar", data, report)?;
        remove_desktop_override(&entry, "quicklook-thunar", report)?;
        if wanted {
            report.notes.push(format!(
                "Thunar 空格接管未启用：缺少 {}（重新运行 scripts/install-local.py 即可）",
                library.display()
            ));
        }
        return Ok(());
    }
    let script = launcher_script(&library, "thunar", "org.xfce.Thunar", "Thunar", "");
    write(&launcher, &script, true, report)?;
    // 覆盖系统自带的 thunar.desktop：同名文件在更靠前的 XDG 目录里优先。
    let desktop = desktop_with_launcher(&system_desktop("thunar.desktop"), "thunar", &launcher)
        .unwrap_or_else(|| minimal_desktop("Thunar", &launcher));
    write_desktop_override(&entry, "quicklook-thunar", &desktop, report)?;
    remove_path_shim("thunar", data, report)?;
    // Publish activation routes only after the executable loader exists.
    apply_thunar_activation(true, &launcher, data, config_home, report)?;
    Ok(())
}

fn apply_thunar_activation(
    wanted: bool,
    launcher: &Path,
    data: &Path,
    config_home: &Path,
    report: &mut Report,
) -> Result<()> {
    for bus in THUNAR_BUSES {
        let service = data.join(format!("dbus-1/services/{bus}.service"));
        if wanted {
            // Omit SystemdService: non-systemd sessions must use our Exec too.
            write_desktop_override(
                &service,
                "quicklook-thunar",
                &format!(
                    "[D-BUS Service]\nName={bus}\nExec={} --gapplication-service\n",
                    json_path(launcher)
                ),
                report,
            )?;
        } else {
            remove_desktop_override(&service, "quicklook-thunar", report)?;
        }
    }
    // Keep the generic org.freedesktop.FileManager1 choice untouched: Dolphin
    // may own it. If the distro chooses Thunar's systemd unit, adapt that unit.
    let drop_in = config_home.join("systemd/user/thunar.service.d/quicklook.conf");
    let changed_at = report.written.len() + report.removed.len();
    if wanted {
        write_desktop_override(
            &drop_in,
            "# quicklook-thunar",
            &format!(
                "# quicklook-thunar: optional per-user Space preview\n[Service]\nExecStart=\nExecStart={} --daemon\n",
                systemd_exec_arg(launcher)
            ),
            report,
        )?;
    } else {
        remove_desktop_override(&drop_in, "# quicklook-thunar", report)?;
    }
    if !report.dry_run
        && changed_at != report.written.len() + report.removed.len()
        && config_home == xdg_config_home()
    {
        // Reload unit definitions only; never start/restart the user's manager.
        use glib::variant::ToVariant;
        if let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) {
            let _ = bus.call_sync(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
                "Reload",
                Some(&().to_variant()),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                3000,
                None::<&gio::Cancellable>,
            );
        }
    }
    Ok(())
}

fn systemd_exec_arg(path: &Path) -> String {
    // systemd expands % specifiers in executable paths. Environment expansion
    // applies only to arguments, so $ in this first token must remain literal.
    let value = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('%', "%%");
    format!("\"{value}\"")
}

/// Caja 的用户扩展加载 Rust 按键适配器，普通 caja 命令也会生效。
/// 没有 python3-caja 的系统仍可通过显式 LD_PRELOAD 启动器接入。
///
/// 与 Thunar 启动器共用 `launcher_script`：宿主已经在跑但没加载适配器时静默重启一次；
/// 加载的是旧版库则保持不动。`--quit` 会关掉正在浏览的窗口，所以只在「完全没加载」
/// 这种空格根本用不了的情况下才走这条路。
fn apply_caja(config: &Config, prefix: &Path, data: &Path, report: &mut Report) -> Result<()> {
    let cli = cli_path(prefix);
    let menu = data.join("caja-python/extensions/quicklook.py");
    let library = prefix.join("lib/quicklook/libquicklook_caja.so");
    if !config.host_caja {
        remove(&menu, report)?;
        return remove_caja_launcher(prefix, data, report);
    }
    write(
        &menu,
        &CAJA_MENU
            .replace("@QUICKLOOK_CLI@", &json_path(&cli))
            .replace("@QUICKLOOK_ADAPTER@", &json_path(&library))
            .replace(
                "@LOAD_ADAPTER@",
                if config.hijack_native && library.is_file() {
                    "True"
                } else {
                    "False"
                },
            ),
        false,
        report,
    )?;
    let launcher = launcher_path(data, "caja");
    let wanted = config.hijack_native;
    if wanted {
        report.notes.push("Caja 普通命令启动的空格预览通过 python3-caja 加载 Rust 适配；缺少该系统扩展时请安装 python3-caja，或使用 quicklook --launch-host caja。".into());
    }
    if !wanted || !library.is_file() {
        remove_caja_launcher(prefix, data, report)?;
        if wanted {
            report.notes.push(format!(
                "Caja 空格接管未启用：缺少 {}（重新运行 scripts/install-local.py 即可）",
                library.display()
            ));
        }
        return Ok(());
    }
    let script = launcher_script(&library, "caja", "org.mate.Caja", "Caja", "");
    write(&launcher, &script, true, report)?;
    let mut replaced = 0usize;
    for name in CAJA_ENTRIES {
        let system = system_desktop(name);
        let Some(desktop) = desktop_with_launcher(&system, "caja", &launcher) else {
            continue;
        };
        write_desktop_override(
            &data.join("applications").join(name),
            "quicklook-caja",
            &desktop,
            report,
        )?;
        replaced += 1;
    }
    if replaced == 0 {
        // 系统条目一个都没读到（非发行版安装，或路径不同），兜一个最小条目。
        let desktop = minimal_desktop("Caja", &launcher);
        write_desktop_override(
            &data.join("applications/caja-browser.desktop"),
            "quicklook-caja",
            &desktop,
            report,
        )?;
    }
    remove_path_shim("caja", data, report)?;
    Ok(())
}

/// 撤销 Caja 的启动器与我们覆盖过的 desktop 条目。
///
/// 这些条目是按系统条目的内容复制出来再改 Exec 的，用户自己可能也写过同名文件，
/// 所以只在内容里确实带着启动器路径时才删。
fn remove_caja_launcher(_prefix: &Path, data: &Path, report: &mut Report) -> Result<()> {
    let launcher = launcher_path(data, "caja");
    remove(&launcher, report)?;
    remove_path_shim("caja", data, report)?;
    for name in CAJA_ENTRIES {
        remove_desktop_override(
            &data.join("applications").join(name),
            "quicklook-caja",
            report,
        )?;
    }
    Ok(())
}

fn apply_accels(
    config: &Config,
    key: &PreviewKey,
    config_home: &Path,
    report: &mut Report,
) -> Result<()> {
    let path = config_home.join("nautilus/scripts-accels");
    let old = read_text(&path)?;
    let mut lines = Vec::new();
    let mut taken = false;
    for line in old.lines() {
        let (accel, item) = split_accel(line);
        if item == SCRIPT_ITEM {
            continue;
        }
        if accelerator_key(accel).as_ref() == Some(key) {
            taken = true;
        }
        lines.push(line.to_string());
    }
    let wanted = config.host_nautilus && config.hijack_native && key.nautilus_bindable();
    if wanted {
        if taken {
            report.notes.push(format!(
                "已有其他 {} 脚本绑定，保留原绑定；可用脚本菜单调用 Quick Look。",
                key
            ));
        } else {
            lines.push(format!("{} {SCRIPT_ITEM}", key.accelerator()));
        }
    }
    let text = lines.join("\n");
    if text.trim().is_empty() {
        remove(&path, report)
    } else {
        write(&path, &(text + "\n"), false, report)
    }
}

fn apply_yazi(
    config: &Config,
    key: &PreviewKey,
    cli: &Path,
    config_home: &Path,
    report: &mut Report,
) -> Result<()> {
    let yazi = config_home.join("yazi");
    let plugin = yazi.join("plugins/quicklook.yazi/main.lua");
    let snippet = yazi.join("quicklook-keymap.toml");
    if !config.host_yazi {
        remove(&plugin, report)?;
        remove(&snippet, report)?;
        let init = yazi.join("init.lua");
        clear_managed(&init, report)?;
        let keymap = yazi.join("keymap.toml");
        clear_managed(&keymap, report)?;
        return Ok(());
    }
    write(
        &plugin,
        &YAZI.replace("@QUICKLOOK_CLI@", &json_path_inner(cli)),
        false,
        report,
    )?;
    apply_yazi_init(cli, &yazi, report)?;
    apply_yazi_keymap(config, key, &yazi, &snippet, report)
}

fn apply_yazi_init(cli: &Path, yazi: &Path, report: &mut Report) -> Result<()> {
    let path = yazi.join("init.lua");
    let old = read_text(&path)?;
    let clean = strip_managed(&old)?;
    let block = if manual_setup(&clean) {
        String::new()
    } else {
        format!(
            "{BEGIN_LUA}\nrequire(\"quicklook\"):setup {{ cli = {} }}\n{END_LUA}\n",
            json_path(cli)
        )
    };
    let desired = join_block(&clean, &block);
    if desired != old.trim_end() {
        write(&path, &desired, false, report)?;
    }
    Ok(())
}

fn apply_yazi_keymap(
    config: &Config,
    key: &PreviewKey,
    yazi: &Path,
    snippet: &Path,
    report: &mut Report,
) -> Result<()> {
    let path = yazi.join("keymap.toml");
    let old = read_text(&path)?;
    let clean = strip_managed(&old)?;
    if !config.hijack_native {
        let desired = join_block(&clean, "");
        if desired != old.trim_end() {
            write(&path, &desired, false, report)?;
        }
        return Ok(());
    }
    let existing = match toml::from_str::<toml::Value>(&clean) {
        Ok(value) => keymap_keys(&value),
        Err(_) => {
            report
                .notes
                .push("Yazi keymap.toml 无法解析，未改动；请手动绑定预览键。".into());
            return Ok(());
        }
    };
    let mut entries = Vec::new();
    let yazi_key = key.yazi();
    if !existing.contains(&yazi_key) {
        entries.push(format!(
            "[[mgr.prepend_keymap]]\non = {}\nrun = \"plugin quicklook\"\ndesc = \"Quick Look 预览\"\n",
            json_text(&yazi_key)
        ));
    }
    if !existing.contains(&"<Esc>".to_string()) {
        entries.push(
            "[[mgr.prepend_keymap]]\non = \"<Esc>\"\nrun = [\"plugin quicklook -- close\", \"escape\"]\ndesc = \"关闭 Quick Look / 原有 Esc\"\n"
                .to_string(),
        );
    }
    if entries.is_empty() {
        let desired = join_block(&clean, "");
        if desired != old.trim_end() {
            write(&path, &desired, false, report)?;
        }
        return Ok(());
    }
    let block = format!("{BEGIN}\n{}\n{END}\n", entries.join("\n"));
    let proposed = join_block(&clean, &block);
    if toml::from_str::<toml::Value>(&proposed).is_err() {
        // 行内数组无法用 array-of-table 语法追加，保留用户文件并给出可合并片段。
        write(snippet, &block, false, report)?;
        report.notes.push(
            "Yazi 已有行内按键数组，保留原配置；请合并 quicklook-keymap.toml 中的条目。".into(),
        );
        return Ok(());
    }
    remove(snippet, report)?;
    write(&path, &proposed, false, report)?;
    Ok(())
}

fn keymap_keys(value: &toml::Value) -> Vec<String> {
    let mut keys = Vec::new();
    let Some(entries) = value
        .get("mgr")
        .and_then(|mgr| mgr.get("prepend_keymap"))
        .and_then(toml::Value::as_array)
    else {
        return keys;
    };
    for entry in entries {
        match entry.get("on") {
            Some(toml::Value::String(text)) => keys.push(text.clone()),
            Some(toml::Value::Array(items)) => {
                keys.extend(
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string)),
                );
            }
            _ => {}
        }
    }
    keys
}

/// 用户自己写过 `require("quicklook")` 时不再追加，避免重复 setup。
fn manual_setup(text: &str) -> bool {
    text.contains("require(\"quicklook\")") || text.contains("require('quicklook')")
}

fn clear_managed(path: &Path, report: &mut Report) -> Result<()> {
    let old = read_text(path)?;
    let clean = strip_managed(&old)?;
    if clean == old.trim_end() {
        return Ok(());
    }
    if clean.is_empty() {
        remove(path, report)
    } else {
        write(path, &(clean + "\n"), false, report)
    }
}

/// 去掉托管标记块，保留其余所有内容。
pub fn strip_managed(text: &str) -> Result<String> {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        match line.trim() {
            BEGIN | BEGIN_LUA | BEGIN_XML => {
                if inside {
                    bail!("托管配置标记嵌套，未改动此文件；请先修复 BEGIN/END 标记");
                }
                inside = true;
            }
            END | END_LUA | END_XML => {
                if !inside {
                    bail!("托管配置有孤立 END，未改动此文件；请先修复标记");
                }
                inside = false;
            }
            _ if inside => {}
            _ => kept.push(line),
        }
    }
    if inside {
        bail!("托管配置缺少 END，未改动此文件；请先修复标记");
    }
    Ok(kept.join("\n").trim_end().to_string())
}

fn join_block(clean: &str, block: &str) -> String {
    let clean = clean.trim_end();
    let block = block.trim_end();
    match (clean.is_empty(), block.is_empty()) {
        (true, true) => String::new(),
        (true, false) => block.to_string(),
        (false, true) => clean.to_string(),
        (false, false) => format!("{clean}\n\n{block}"),
    }
}

fn split_accel(line: &str) -> (&str, &str) {
    match line.split_once(char::is_whitespace) {
        Some((accel, item)) => (accel, item.trim()),
        None => ("", ""),
    }
}

fn accelerator_key(accel: &str) -> Option<PreviewKey> {
    PreviewKey::parse(&accel.replace('<', "").replace('>', "+"))
}

/// 图标是静态资源，不进 Report：用户看到的「写入 N 个文件」只该反映集成开关。
fn apply_icons(data: &Path, report: &Report) -> Result<()> {
    for (relative, bytes) in ICONS {
        let path = data.join("icons/hicolor").join(relative);
        if path.is_file() && std::fs::read(&path).ok().as_deref() == Some(*bytes) {
            continue;
        }
        if report.dry_run {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    if !report.dry_run {
        // 无条件重建：缓存可能被旧版本安装、桌面环境升级之类的事弄脏，
        // 而这一步只有几毫秒，顺手做掉比留着让人排查半天划算。
        refresh_icon_cache(&data.join("icons/hicolor"));
    }
    Ok(())
}

/// 光把 PNG 写进 XDG 目录还不够：桌面环境查图标用的是 `icon-theme.cache`，
/// 缓存没重建时按名字找不到新图标，托盘和桌面条目都会退回宿主的兜底图形。
/// 这正是本机排查时踩到的坑——图标文件早在，缓存停在两周前。
fn refresh_icon_cache(directory: &Path) {
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("-f")
        .arg("-t")
        .arg("-q")
        .arg(directory)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Dolphin 的服务菜单不是每次打开目录都重新扫盘，而是读 `ksycoca` 缓存。
/// 只写 `.desktop` 不重建缓存时，右键菜单要等到别的原因触发重建才出现，
/// 中间这段时间用户看到的就是「装了但没集成」。只在真实数据目录上执行，
/// 测试和预演（临时目录/`dry_run`）不会碰用户缓存。
fn refresh_kde_service_cache(data: &Path) {
    if data != xdg_data_home() {
        return;
    }
    for tool in ["kbuildsycoca5", "kbuildsycoca6", "kbuildsycoca"] {
        let result = std::process::Command::new(tool)
            .arg("--noincremental")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match result {
            Ok(_) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return,
        }
    }
}

fn read_text(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    Ok(std::fs::read_to_string(path)?)
}

fn write(path: &Path, text: &str, executable: bool, report: &mut Report) -> Result<()> {
    if path.is_file() && std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        if executable && !report.dry_run {
            set_executable(path)?;
        }
        return Ok(());
    }
    report.written.push(path.to_path_buf());
    if report.dry_run {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 先写临时文件再改名：中途失败不会留下半截配置，宿主也能读到完整内容。
    let temporary = path.with_file_name(format!(
        ".{}.quicklook-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tmp"),
        std::process::id()
    ));
    std::fs::write(&temporary, text)?;
    if executable {
        set_executable(&temporary)?;
    }
    std::fs::rename(&temporary, path)?;
    Ok(())
}

fn remove(path: &Path, report: &mut Report) -> Result<()> {
    if path.symlink_metadata().is_ok() {
        report.removed.push(path.to_path_buf());
        if report.dry_run {
            return Ok(());
        }
        std::fs::remove_file(path)?;
        if let Some(parent) = path.parent()
            && parent
                .read_dir()
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

/// 只删除内容里带我们标记的文件。`nemo.desktop` / `thunar.desktop` 这类路径
/// 用户也可能自己写过，不能凭路径就删。
fn remove_owned(path: &Path, marker: &str, report: &mut Report) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if text.contains(marker) {
        remove(path, report)
    } else {
        Ok(())
    }
}

fn original_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.quicklook-original",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("desktop")
    ))
}

/// 覆盖系统 desktop 条目前先保住用户自己写过的那份。
///
/// 这些路径（`~/.local/share/applications/*.desktop`）是 XDG 优先级最高的位置，
/// 用户为了改图标、加参数很可能写过同名文件；不备份就直接覆盖，卸载时也找不回来。
fn write_desktop_override(
    path: &Path,
    marker: &str,
    text: &str,
    report: &mut Report,
) -> Result<()> {
    let already_ours = std::fs::read_to_string(path)
        .ok()
        .is_some_and(|existing| existing.contains(marker));
    if path.is_file() && !already_ours {
        let original = original_path(path);
        if !original.exists() {
            report.written.push(original.clone());
            if !report.dry_run {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(path, &original)?;
            }
            report.notes.push(format!(
                "用户原有的 {} 已备份到 {}",
                path.display(),
                original.display()
            ));
        }
    }
    write(path, text, false, report)
}

/// 撤销我们写过的 desktop 覆盖，并把备份的用户原文件放回去。
fn remove_desktop_override(path: &Path, marker: &str, report: &mut Report) -> Result<()> {
    remove_owned(path, marker, report)?;
    let original = original_path(path);
    if !original.is_file() || path.exists() {
        return Ok(());
    }
    report.removed.push(original.clone());
    if !report.dry_run {
        std::fs::rename(&original, path)?;
    }
    Ok(())
}

fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

fn install_prefix() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("无法定位程序路径")?;
    let bin = exe.parent().context("程序路径异常")?;
    Ok(bin
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| bin.to_path_buf()))
}

fn cli_path(prefix: &Path) -> PathBuf {
    let candidate = prefix.join("bin/quicklook");
    if candidate.is_file() {
        candidate
    } else {
        std::env::current_exe().unwrap_or(candidate)
    }
}

fn bridge_path(prefix: &Path) -> PathBuf {
    let candidate = prefix.join("bin/quicklook-nautilus-bridge");
    if candidate.is_file() {
        candidate
    } else {
        PathBuf::from("quicklook-nautilus-bridge")
    }
}

/// XDG autostart 条目。放在 `$XDG_CONFIG_HOME/autostart` 是规范位置，
/// niri（以 niri-session 运行时）、GNOME、KDE、Xfce 都会读它。
fn autostart_file(config_home: &Path) -> PathBuf {
    config_home.join("autostart/org.quicklook.Preview.desktop")
}

/// 条目只启动常驻 broker：它负责通知栏图标、把首次预览预热到几十毫秒，
/// 预览窗口本身仍然按需拉起、用完就退，不该在登录时占内存。
fn autostart_entry(prefix: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Quick Look\n\
         Comment=常驻预览服务与通知栏图标\n\
         Exec=\"{}\"\n\
         Icon=org.quicklook.Preview\n\
         Terminal=false\n\
         Categories=Utility;\n\
         X-GNOME-Autostart-enabled=true\n",
        desktop_exec_arg(&broker_path(prefix))
    )
}

fn apply_autostart(
    config: &Config,
    config_home: &Path,
    prefix: &Path,
    report: &mut Report,
) -> Result<()> {
    let path = autostart_file(config_home);
    if config.autostart {
        write(&path, &autostart_entry(prefix), false, report)
    } else {
        remove(&path, report)
    }
}

/// 设置窗口的「保存设置」不该顺带改文件管理器接入，但自启条目只是文件读写，
/// 每次保存都对齐一次最省事，也不会打扰任何人。
pub fn apply_autostart_only(config: &Config) -> Result<Report> {
    let prefix = install_prefix()?;
    let mut report = Report::default();
    apply_autostart(config, &xdg_config_home(), &prefix, &mut report)?;
    Ok(report)
}

fn broker_path(prefix: &Path) -> PathBuf {
    let candidate = prefix.join("bin/quicklookd");
    if candidate.is_file() {
        return candidate;
    }
    // 开发构建里 quicklookd 与 quicklook 同目录，不在 <prefix>/bin；
    // autostart / D-Bus 激活环境的 PATH 未必可靠，能写绝对路径就别写裸名字。
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join("quicklookd");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("quicklookd")
}

fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,=:@%".contains(c))
    {
        return text.into_owned();
    }
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Desktop Entry 的字符串转义先于 Exec 参数解析，因此要转义两遍。
fn desktop_exec_arg(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut escaped = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '"' | '`' | '$') {
            escaped.push('\\');
        }
        if c == '%' {
            escaped.push('%');
        }
        escaped.push(c);
    }
    escaped.replace('\\', "\\\\")
}

/// 给路径套上 JSON 引号。
fn json_path(value: &Path) -> String {
    format!("\"{}\"", json_path_inner(value))
}

fn json_path_inner(value: &Path) -> String {
    json_escape(&value.to_string_lossy())
}

fn json_text(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn json_escape(value: &str) -> String {
    let mut text = String::new();
    for c in value.chars() {
        match c {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            c if (c as u32) < 0x20 => text.push_str(&format!("\\u{:04x}", c as u32)),
            c => text.push(c),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    fn enabled() -> Config {
        Config {
            hijack_native: true,
            ..Config::default()
        }
    }
    /// 写一个伪 nemo 二进制并返回路径；调用 apply_for_test 时显式传入，
    /// 让改键→装旁路器的完整链路在没有真实 Nemo 的机器（CI）上也能验证。
    fn install_fake_nemo(prefix: &std::path::Path) -> (std::path::PathBuf, u64) {
        let path = prefix.join("fake-nemo");
        let bytes = b"libnemo placeholder for fingerprint test";
        std::fs::write(&path, bytes).unwrap();
        (path, fnv1a(bytes))
    }

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin/quicklook"), "#!/bin/sh\n").unwrap();
        std::fs::write(prefix.join("bin/quicklookd"), "#!/bin/sh\n").unwrap();
        std::fs::write(prefix.join("bin/quicklook-nautilus-bridge"), "#!/bin/sh\n").unwrap();
        std::fs::create_dir_all(prefix.join("lib/quicklook")).unwrap();
        for name in [
            "libquicklook_thunar.so",
            "libquicklook_nemo.so",
            "libquicklook_caja.so",
        ] {
            std::fs::write(prefix.join("lib/quicklook").join(name), b"\x7fELF").unwrap();
        }
        let data = root.path().join("data");
        let config_home = root.path().join("config");
        (root, prefix, data, config_home)
    }
    #[test]
    fn broken_managed_markers_cannot_delete_user_configuration() {
        let (_root, prefix, data, config_home) = setup();
        let yazi = config_home.join("yazi");
        std::fs::create_dir_all(&yazi).unwrap();
        let old = "-- BEGIN quicklook managed integration\nrequire(\"user-plugin\")\n";
        std::fs::write(yazi.join("init.lua"), old).unwrap();
        assert!(apply_at(&enabled(), &prefix, &data, &config_home).is_err());
        assert_eq!(std::fs::read_to_string(yazi.join("init.lua")).unwrap(), old);
        assert!(!data.exists());
    }

    #[test]
    fn removing_thunar_integration_keeps_actions_with_attributes() {
        let (_root, prefix, data, config_home) = setup();
        let path = config_home.join("Thunar/uca.xml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original =
            "<actions>\n<action custom=\"true\"><name>User</name></action>\n</actions>\n";
        std::fs::write(&path, original).unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        apply_at(
            &Config {
                host_thunar: false,
                ..enabled()
            },
            &prefix,
            &data,
            &config_home,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn native_service_override_is_restored_and_never_deleted_by_default() {
        let (_root, prefix, data, config_home) = setup();
        let path = data.join("dbus-1/services/org.gnome.NautilusPreviewer.service");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = "[D-BUS Service]\nName=org.gnome.NautilusPreviewer\nExec=/usr/bin/sushi\n";
        std::fs::write(&path, old).unwrap();
        apply_at(&Config::default(), &prefix, &data, &config_home).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        apply_at(&Config::default(), &prefix, &data, &config_home).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
    }

    #[test]
    fn custom_accelerator_conflicts_are_detected() {
        assert_eq!(
            accelerator_key("<Shift><Primary>F5"),
            PreviewKey::parse("Ctrl+Shift+F5")
        );
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("nautilus")).unwrap();
        let path = config_home.join("nautilus/scripts-accels");
        std::fs::write(&path, "<Alt>F3 user-script\n").unwrap();
        let config = Config {
            preview_key_nautilus: "Alt+F3".into(),
            ..enabled()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "<Alt>F3 user-script\n"
        );
    }

    #[test]
    fn applies_every_host_entry_point() {
        let (_root, prefix, data, config_home) = setup();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(report.written.len() >= 6);
        let dolphin =
            std::fs::read_to_string(data.join("kio/servicemenus/quicklook.desktop")).unwrap();
        assert!(dolphin.contains(&prefix.join("bin/quicklook").to_string_lossy().to_string()));
        let script = std::fs::read_to_string(data.join("nautilus/scripts/Quick Look")).unwrap();
        assert!(script.starts_with("#!/bin/sh\nexec "));
        assert!(script.ends_with("--from-nautilus\n"));
        let service = std::fs::read_to_string(
            data.join("dbus-1/services/org.gnome.NautilusPreviewer.service"),
        )
        .unwrap();
        assert!(service.contains("quicklook-nautilus-bridge"));
        // Nemo 走它自己的原生预览接口，不需要注入。
        let nemo_service =
            std::fs::read_to_string(data.join("dbus-1/services/org.nemo.Preview.service")).unwrap();
        assert!(nemo_service.contains("Name=org.nemo.Preview"));
        assert!(nemo_service.contains("quicklookd"));
        assert!(nemo_service.contains("--nemo-compat"));
        let nemo =
            std::fs::read_to_string(data.join("nemo/actions/quicklook.nemo_action")).unwrap();
        assert!(nemo.contains("[Nemo Action]"));
        assert!(nemo.contains("%U"));
        assert!(nemo.contains(&prefix.join("bin/quicklook").to_string_lossy().to_string()));
        // 空格没有修饰键，scripts-accels 只收 Ctrl/Alt，故此时由 Previewer2 接管。
        assert!(!config_home.join("nautilus/scripts-accels").exists());
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(keymap.starts_with(BEGIN));
        assert!(keymap.contains("on = \"<Space>\""));
    }
    #[test]
    fn second_apply_is_a_no_op() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(report.written.is_empty(), "{:?}", report.written);
        assert!(report.removed.is_empty());
    }
    #[test]
    fn disabling_hosts_removes_only_our_files() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        std::fs::create_dir_all(config_home.join("yazi")).unwrap();
        std::fs::write(
            config_home.join("yazi/keymap.toml"),
            "[[mgr.prepend_keymap]]\non = \"<C-y>\"\nrun = \"escape\"\n",
        )
        .unwrap();
        let config = Config {
            host_nemo: false,
            host_dolphin: false,
            host_nautilus: false,
            host_yazi: false,
            ..enabled()
        };
        let report = apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(!data.join("kio/servicemenus/quicklook.desktop").exists());
        assert!(!data.join("nemo/actions/quicklook.nemo_action").exists());
        assert!(
            !data
                .join("dbus-1/services/org.nemo.Preview.service")
                .exists()
        );
        assert!(!data.join("nautilus/scripts/Quick Look").exists());
        assert!(
            !config_home
                .join("yazi/plugins/quicklook.yazi/main.lua")
                .exists()
        );
        assert!(!report.removed.is_empty());
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(keymap.contains("<C-y>"));
        assert!(!keymap.contains("plugin quicklook"));
    }
    #[test]
    fn hijack_off_keeps_native_preview_untouched() {
        let (_root, prefix, data, config_home) = setup();
        let config = Config {
            preview_key: "Ctrl+Space".into(),
            ..enabled()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(
            data.join("dbus-1/services/org.gnome.NautilusPreviewer.service")
                .exists()
        );
        let config = Config {
            hijack_native: false,
            ..config
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(
            !data
                .join("dbus-1/services/org.gnome.NautilusPreviewer.service")
                .exists()
        );
        assert!(
            !data
                .join("dbus-1/services/org.nemo.Preview.service")
                .exists()
        );
        assert!(!config_home.join("nautilus/scripts-accels").exists());
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(!keymap.contains("plugin quicklook"));
        // 插件本身仍然安装，用户可自行绑定。
        assert!(
            config_home
                .join("yazi/plugins/quicklook.yazi/main.lua")
                .exists()
        );
    }
    #[test]
    fn custom_key_reaches_every_host() {
        let (_root, prefix, data, config_home) = setup();
        let config = Config {
            preview_key: "Ctrl+Space".into(),
            ..enabled()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(keymap.contains("on = \"<C-Space>\""));
        let accels = std::fs::read_to_string(config_home.join("nautilus/scripts-accels")).unwrap();
        assert_eq!(accels.trim(), "<Control>space Quick Look");
    }
    #[test]
    fn per_host_keys_override_only_their_own_host() {
        let (_root, prefix, data, config_home) = setup();
        let (fake_nemo, fake_fp) = install_fake_nemo(&prefix);
        let config = Config {
            preview_key: "Ctrl+Space".into(),
            preview_key_nemo: "F3".into(),
            preview_key_yazi: "F6".into(),
            preview_key_nautilus: "Alt+F7".into(),
            ..enabled()
        };
        apply_for_test(
            &config,
            &prefix,
            &data,
            &config_home,
            Some(&fake_nemo),
            fake_fp,
        )
        .unwrap();
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(keymap.contains("on = \"<F6>\""), "{keymap}");
        assert!(!keymap.contains("<C-Space>"), "{keymap}");
        let accels = std::fs::read_to_string(config_home.join("nautilus/scripts-accels")).unwrap();
        assert_eq!(accels.trim(), "<Alt>F7 Quick Look");
        // Nemo 的覆盖键不是空格，必须装上按键旁路器。
        assert!(launcher_path(&data, "nemo").is_file());
        // 清空覆盖即回到跟随全局。
        let following = Config {
            preview_key_nemo: String::new(),
            preview_key_yazi: String::new(),
            preview_key_nautilus: String::new(),
            ..config.clone()
        };
        apply_for_test(
            &following,
            &prefix,
            &data,
            &config_home,
            Some(&fake_nemo),
            fake_fp,
        )
        .unwrap();
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(keymap.contains("on = \"<C-Space>\""), "{keymap}");
        let accels = std::fs::read_to_string(config_home.join("nautilus/scripts-accels")).unwrap();
        assert_eq!(accels.trim(), "<Control>space Quick Look");
    }
    #[test]
    fn account_for_existing_control_space_binding() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("nautilus")).unwrap();
        std::fs::write(
            config_home.join("nautilus/scripts-accels"),
            "<Primary>space 其他脚本\n",
        )
        .unwrap();
        let config = Config {
            preview_key: "Ctrl+Space".into(),
            ..enabled()
        };
        let report = apply_at(&config, &prefix, &data, &config_home).unwrap();
        let accels = std::fs::read_to_string(config_home.join("nautilus/scripts-accels")).unwrap();
        assert_eq!(accels.trim(), "<Primary>space 其他脚本");
        assert!(report.notes.iter().any(|n| n.contains("Ctrl+Space")));
    }
    #[test]
    fn inline_keymap_falls_back_to_snippet() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("yazi")).unwrap();
        std::fs::write(
            config_home.join("yazi/keymap.toml"),
            "mgr = { prepend_keymap = [ { on = \"<C-y>\", run = \"escape\" } ] }\n",
        )
        .unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(config_home.join("yazi/quicklook-keymap.toml").exists());
        assert!(report.notes.iter().any(|n| n.contains("行内")));
        let keymap = std::fs::read_to_string(config_home.join("yazi/keymap.toml")).unwrap();
        assert!(!keymap.contains("plugin quicklook"));
    }
    #[test]
    fn keeps_user_lua_and_adds_managed_block_once() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("yazi")).unwrap();
        std::fs::write(
            config_home.join("yazi/init.lua"),
            "-- mine\nrequire(\"full-border\")\n",
        )
        .unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let init = std::fs::read_to_string(config_home.join("yazi/init.lua")).unwrap();
        assert!(init.starts_with("-- mine\nrequire(\"full-border\")"));
        assert_eq!(init.matches("quicklook\")").count(), 1);
    }
    #[test]
    fn manual_setup_is_not_duplicated() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("yazi")).unwrap();
        let manual = "require(\"quicklook\"):setup { cli = \"/opt/quicklook\" }\n";
        std::fs::write(config_home.join("yazi/init.lua"), manual).unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let init = std::fs::read_to_string(config_home.join("yazi/init.lua")).unwrap();
        assert_eq!(init, manual);
    }
    #[test]
    fn strip_managed_handles_both_comment_styles() {
        let text = "a\n# BEGIN quicklook managed integration\nx\n# END quicklook managed integration\nb\n-- BEGIN quicklook managed integration\ny\n-- END quicklook managed integration\nc";
        assert_eq!(strip_managed(text).unwrap(), "a\nb\nc");
    }
    #[test]
    fn strip_managed_handles_xml_comments() {
        let text = "<actions>\n<!-- BEGIN quicklook managed integration -->\n<action/>\n<!-- END quicklook managed integration -->\n</actions>";
        assert_eq!(strip_managed(text).unwrap(), "<actions>\n</actions>");
    }
    #[test]
    fn thunar_action_is_created_with_root() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let uca = std::fs::read_to_string(config_home.join("Thunar/uca.xml")).unwrap();
        assert!(uca.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>"));
        assert!(uca.trim_end().ends_with("</actions>"));
        assert!(uca.contains("<unique-id>quicklook-preview</unique-id>"));
        assert!(uca.contains(&format!(
            "{} --show %F",
            prefix.join("bin/quicklook").to_string_lossy()
        )));
    }
    #[test]
    fn thunar_keeps_user_actions() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("Thunar")).unwrap();
        let mine = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n  <action>\n    <name>我的动作</name>\n  </action>\n</actions>\n";
        std::fs::write(config_home.join("Thunar/uca.xml"), mine).unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let uca = std::fs::read_to_string(config_home.join("Thunar/uca.xml")).unwrap();
        assert!(uca.contains("我的动作"));
        assert!(uca.contains("quicklook-preview"));
        // 我们的动作必须在根元素内部，否则整个文件失效。
        assert!(uca.find("quicklook-preview").unwrap() < uca.find("</actions>").unwrap());
        assert_eq!(uca.matches("<actions>").count(), 1);
    }
    #[test]
    fn thunar_second_apply_is_noop() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(report.written.is_empty(), "第二次不应再写文件");
        let uca = std::fs::read_to_string(config_home.join("Thunar/uca.xml")).unwrap();
        assert_eq!(uca.matches("quicklook-preview").count(), 1);
    }
    #[test]
    fn thunar_disable_keeps_user_actions_and_drops_ours() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("Thunar")).unwrap();
        let mine = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n  <action>\n    <name>我的动作</name>\n  </action>\n</actions>\n";
        std::fs::write(config_home.join("Thunar/uca.xml"), mine).unwrap();
        let mut config = enabled();
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        config.host_thunar = false;
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        let uca = std::fs::read_to_string(config_home.join("Thunar/uca.xml")).unwrap();
        assert!(uca.contains("我的动作"));
        assert!(!uca.contains("quicklook-preview"));
        assert!(!uca.contains("managed integration"));
    }
    #[test]
    fn thunar_disable_removes_file_we_created() {
        let (_root, prefix, data, config_home) = setup();
        let mut config = enabled();
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(config_home.join("Thunar/uca.xml").exists());
        config.host_thunar = false;
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(!config_home.join("Thunar/uca.xml").exists());
    }
    #[test]
    fn thunar_notes_broken_file_instead_of_clobbering() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("Thunar")).unwrap();
        let broken = "<?xml version=\"1.0\"?>\n<actions>\n  <action>\n";
        std::fs::write(config_home.join("Thunar/uca.xml"), broken).unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(report.notes.iter().any(|n| n.contains("</actions>")));
        assert_eq!(
            std::fs::read_to_string(config_home.join("Thunar/uca.xml")).unwrap(),
            broken
        );
    }

    #[test]
    fn thunar_adapter_installs_launcher_and_desktop_entry() {
        let (_root, prefix, data, config_home) = setup();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let launcher = launcher_path(&data, "thunar");
        assert!(launcher.is_file());
        let script = std::fs::read_to_string(&launcher).unwrap();
        assert!(script.contains("LD_PRELOAD="));
        assert!(
            script.contains(
                &prefix
                    .join("lib/quicklook/libquicklook_thunar.so")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert!(script.contains("/usr/bin/thunar \"$@\""));
        // 只看路径会把「重装覆盖后仍映射旧 inode」的实例误判为已适配。
        assert!(script.contains("stat -Lc %i"));
        assert!(script.contains("current_inode"));
        let entry = std::fs::read_to_string(data.join("applications/thunar.desktop")).unwrap();
        assert!(entry.contains("Exec="));
        assert!(entry.contains("quicklook-thunar"));
        assert!(entry.contains("MimeType=inode/directory;"));
        assert!(
            report.written.iter().any(|path| path == &launcher),
            "{:?}",
            report.written
        );
    }

    #[test]
    fn thunar_adapter_disabled_by_hijack_switch() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let config = Config {
            hijack_native: false,
            ..enabled()
        };
        let report = apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(!launcher_path(&data, "thunar").exists());
        assert!(!data.join("applications/thunar.desktop").exists());
        for bus in THUNAR_BUSES {
            assert!(!data.join(format!("dbus-1/services/{bus}.service")).exists());
        }
        assert!(
            !config_home
                .join("systemd/user/thunar.service.d/quicklook.conf")
                .exists()
        );
        // 右键动作与宿主开关无关，仍然留着。
        assert!(config_home.join("Thunar/uca.xml").is_file());
        assert!(
            report
                .removed
                .iter()
                .any(|path| path.ends_with("quicklook-thunar"))
        );
    }

    #[test]
    fn thunar_activation_routes_restore_existing_user_configuration() {
        let (_root, prefix, data, config_home) = setup();
        let service = data.join("dbus-1/services/org.xfce.Thunar.service");
        let drop_in = config_home.join("systemd/user/thunar.service.d/quicklook.conf");
        std::fs::create_dir_all(service.parent().unwrap()).unwrap();
        std::fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
        let old_service = "[D-BUS Service]\nName=org.xfce.Thunar\nExec=/opt/my-thunar\n";
        let old_unit = "[Service]\nEnvironment=MY_OPTION=yes\n";
        std::fs::write(&service, old_service).unwrap();
        std::fs::write(&drop_in, old_unit).unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        for bus in THUNAR_BUSES {
            let text = std::fs::read_to_string(data.join(format!("dbus-1/services/{bus}.service")))
                .unwrap();
            assert!(text.contains(&format!("Name={bus}\n")));
            assert!(text.contains("quicklook-thunar\" --gapplication-service"));
            assert!(!text.contains("SystemdService="));
        }
        let unit = std::fs::read_to_string(&drop_in).unwrap();
        assert!(unit.contains("ExecStart=\nExecStart="));
        assert!(unit.contains("quicklook-thunar\" --daemon"));
        // Do not compete with the user's generic (possibly Dolphin) file manager.
        assert!(
            !data
                .join("dbus-1/services/org.freedesktop.FileManager1.service")
                .exists()
        );
        assert!(
            !data
                .join("dbus-1/services/org.xfce.Thunar.FileManager1.service")
                .exists()
        );
        apply_at(
            &Config {
                hijack_native: false,
                ..enabled()
            },
            &prefix,
            &data,
            &config_home,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&service).unwrap(), old_service);
        assert_eq!(std::fs::read_to_string(&drop_in).unwrap(), old_unit);
        assert!(!original_path(&service).exists());
        assert!(!original_path(&drop_in).exists());
    }

    #[test]
    fn systemd_launcher_path_escapes_specifiers_and_keeps_literal_dollars() {
        assert_eq!(
            systemd_exec_arg(Path::new("/tmp/a %i $HOME/\"q\\l\n")),
            "\"/tmp/a %%i $HOME/\\\"q\\\\l\\n\""
        );
    }

    /// Caja 扩展同时负责右键和 Rust 适配引导；显式启动器仍可用。
    #[test]
    fn caja_writes_menu_extension_launcher_and_entries() {
        let (_root, prefix, data, config_home) = setup();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let menu = data.join("caja-python/extensions/quicklook.py");
        let python = std::fs::read_to_string(&menu).unwrap();
        assert!(python.contains("class QuickLookProvider"));
        // 两条查找路径都要覆盖：caja-python 认属性名，GObject 内省认 vfunc 名。
        assert!(python.contains("def get_file_items"));
        assert!(python.contains("def do_get_file_items"));
        assert!(python.contains("LOAD_ADAPTER = True"));
        assert!(python.contains("quicklook_caja_install"));
        assert!(!python.contains("@QUICKLOOK_ADAPTER@"));
        assert!(python.contains(&format!(
            "CLI = {}",
            json_path(&prefix.join("bin/quicklook"))
        )));
        let launcher = launcher_path(&data, "caja");
        assert!(launcher.is_file());
        let script = std::fs::read_to_string(&launcher).unwrap();
        assert!(script.contains("LD_PRELOAD="));
        assert!(script.contains("/usr/bin/caja \"$@\""));
        // 重启走宿主自己的 --quit，不发信号，且没退干净就不继续。
        assert!(script.contains("/usr/bin/caja --quit"));
        assert!(!script.contains("pkill"));
        assert!(report.written.iter().any(|path| path == &menu));
        // 系统条目在不在都该有一个 applications 覆盖指向启动器。
        let overrides: Vec<String> = std::fs::read_dir(data.join("applications"))
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(entry.path()).ok()?;
                (name.starts_with("caja") && text.contains("quicklook-caja")).then_some(name)
            })
            .collect();
        assert!(!overrides.is_empty(), "{overrides:?}");
    }

    #[test]
    fn caja_extension_preserves_menu_without_enabling_key_takeover() {
        let (_root, prefix, data, config_home) = setup();
        let config = Config {
            hijack_native: false,
            ..enabled()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        let menu = data.join("caja-python/extensions/quicklook.py");
        assert!(
            std::fs::read_to_string(&menu)
                .unwrap()
                .contains("LOAD_ADAPTER = False")
        );
        assert!(!launcher_path(&data, "caja").exists());
        // A missing optional library must leave the menu working too.
        std::fs::remove_file(prefix.join("lib/quicklook/libquicklook_caja.so")).unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(
            std::fs::read_to_string(&menu)
                .unwrap()
                .contains("LOAD_ADAPTER = False")
        );
    }

    /// 关掉开关时只撤销自己写的东西：用户自己动过同名 desktop 条目不能跟着删。
    #[test]
    fn disabling_caja_keeps_user_desktop_entries() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let entry = data.join("applications/caja-browser.desktop");
        std::fs::write(
            &entry,
            "[Desktop Entry]\nType=Application\nName=我的文件\nExec=caja %U\n",
        )
        .unwrap();
        let config = Config {
            host_caja: false,
            ..enabled()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(!launcher_path(&data, "caja").exists());
        assert!(!data.join("caja-python/extensions/quicklook.py").exists());
        assert!(entry.exists());
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains("我的文件")
        );
    }

    /// 覆盖系统条目前先备份用户自己写过的同名文件，撤销时原样放回去。
    #[test]
    fn desktop_override_backs_up_and_restores_the_users_entry() {
        let (_root, prefix, data, config_home) = setup();
        let applications = data.join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        let entry = applications.join("thunar.desktop");
        std::fs::write(
            &entry,
            "[Desktop Entry]\nType=Application\nName=我的 Thunar\nExec=thunar --my-flag\n",
        )
        .unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let backup = applications.join("thunar.desktop.quicklook-original");
        assert!(backup.is_file());
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains("quicklook-thunar")
        );
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("已备份") && note.contains("thunar.desktop"))
        );
        // 关掉接管：我们的覆盖撤掉，用户那份原样回来，备份不残留。
        let config = Config {
            hijack_native: false,
            ..enabled()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains("我的 Thunar")
        );
        assert!(!backup.exists());
    }

    #[test]
    fn integration_does_not_shadow_file_manager_commands() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        for program in ["caja", "thunar"] {
            assert!(!data.parent().unwrap().join("bin").join(program).exists());
            assert!(launcher_path(&data, program).is_file());
        }
    }

    #[test]
    fn caja_without_adapter_library_keeps_space_unhijacked() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::remove_file(prefix.join("lib/quicklook/libquicklook_caja.so")).unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(!launcher_path(&data, "caja").exists());
        // 右键菜单不依赖适配器，库缺失时照样写。
        assert!(data.join("caja-python/extensions/quicklook.py").is_file());
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("Caja 空格接管未启用")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn thunar_adapter_notes_missing_library() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::remove_file(prefix.join("lib/quicklook/libquicklook_thunar.so")).unwrap();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(!launcher_path(&data, "thunar").exists());
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("Thunar 空格接管未启用")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn desktop_override_only_rewrites_exec_lines() {
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("nemo.desktop");
        std::fs::write(
            &system,
            "[Desktop Entry]\nName=Files\nName[zh_CN]=文件\nExec=nemo %U\nIcon=system-file-manager\nActions=open-home;\n\n[Desktop Action open-home]\nName=Home\nExec=nemo computer:///\n",
        )
        .unwrap();
        let launcher = PathBuf::from("/opt/ql/bin/quicklook-nemo");
        let text = desktop_with_launcher(&system, "nemo", &launcher).unwrap();
        // 两个 Exec 都换成启动器，其余内容（多语言名字、Actions）原样保留。
        assert_eq!(
            text.matches("Exec=\"/opt/ql/bin/quicklook-nemo\"").count(),
            2
        );
        assert!(!text.contains("Exec=nemo"));
        assert!(text.contains("Name[zh_CN]=文件"));
        assert!(text.contains("Actions=open-home;"));
        assert!(text.contains("Exec=\"/opt/ql/bin/quicklook-nemo\" computer:///"));
    }

    #[test]
    fn desktop_override_returns_none_without_matching_exec() {
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("other.desktop");
        std::fs::write(&system, "[Desktop Entry]\nExec=/usr/bin/other %U\n").unwrap();
        assert!(desktop_with_launcher(&system, "nemo", Path::new("/x/quicklook-nemo")).is_none());
    }

    #[test]
    fn nemo_uses_native_preview_service() {
        let (_root, prefix, data, config_home) = setup();
        let report = apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let service = data.join("dbus-1/services/org.nemo.Preview.service");
        let text = std::fs::read_to_string(&service).unwrap();
        assert!(text.contains("Name=org.nemo.Preview"));
        assert!(text.contains(&format!(
            "Exec=\"{}\" --nemo-compat",
            prefix.join("bin/quicklookd").to_string_lossy()
        )));
        assert!(
            report.written.iter().any(|path| path == &service),
            "{:?}",
            report.written
        );
        // 旧方案的注入启动器与 desktop 覆盖不应再出现。
        assert!(!launcher_path(&data, "nemo").exists());
        assert!(!data.join("applications/nemo.desktop").exists());
    }

    #[test]
    fn nemo_custom_key_installs_interceptor_and_restores_default() {
        let (_root, prefix, data, config_home) = setup();
        let (fake_nemo, fake_fp) = install_fake_nemo(&prefix);
        let config = Config {
            preview_key: "Ctrl+Space".into(),
            ..enabled()
        };
        let report = apply_for_test(
            &config,
            &prefix,
            &data,
            &config_home,
            Some(&fake_nemo),
            fake_fp,
        )
        .unwrap();
        let launcher = launcher_path(&data, "nemo");
        let script = std::fs::read_to_string(&launcher).unwrap();
        assert!(script.contains("LD_PRELOAD="));
        assert!(
            script.contains(
                &prefix
                    .join("lib/quicklook/libquicklook_nemo.so")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert!(script.contains("QUICKLOOK_NEMO_FINGERPRINT="));
        assert!(script.contains("nemo --quit"));
        assert!(script.contains("/usr/bin/nemo"));
        let desktop = std::fs::read_to_string(data.join("applications/nemo.desktop")).unwrap();
        assert!(desktop.contains(&launcher.to_string_lossy().to_string()));
        // 原生服务仍保留：未走启动器的旧实例还有空格兜底。
        assert!(
            data.join("dbus-1/services/org.nemo.Preview.service")
                .is_file()
        );
        assert!(
            report.written.iter().any(|path| path == &launcher),
            "{:?}",
            report.written
        );

        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(!launcher.exists());
        assert!(!data.join("applications/nemo.desktop").exists());
        assert!(
            data.join("dbus-1/services/org.nemo.Preview.service")
                .is_file()
        );
    }

    #[test]
    fn nemo_custom_key_without_adapter_keeps_native_space_and_reports() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::remove_file(prefix.join("lib/quicklook/libquicklook_nemo.so")).unwrap();
        let config = Config {
            preview_key: "F5".into(),
            ..enabled()
        };
        let report = apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(!launcher_path(&data, "nemo").exists());
        assert!(!data.join("applications/nemo.desktop").exists());
        assert!(
            data.join("dbus-1/services/org.nemo.Preview.service")
                .is_file()
        );
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("Nemo 快捷键接管未启用")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn nemo_native_preview_disabled_by_hijack_switch() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let config = Config {
            hijack_native: false,
            ..enabled()
        };
        let report = apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(
            !data
                .join("dbus-1/services/org.nemo.Preview.service")
                .exists()
        );
        assert!(!launcher_path(&data, "nemo").exists());
        assert!(!data.join("applications/nemo.desktop").exists());
        // 右键动作不依赖原生预览，仍然保留。
        assert!(data.join("nemo/actions/quicklook.nemo_action").is_file());
        assert!(
            report
                .removed
                .iter()
                .any(|path| path.ends_with("org.nemo.Preview.service"))
        );
    }

    #[test]
    fn old_nemo_desktop_override_is_cleaned_up_but_user_files_are_kept() {
        let (_root, prefix, data, config_home) = setup();
        let applications = data.join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        let ours = applications.join("nemo.desktop");
        std::fs::write(
            &ours,
            "[Desktop Entry]\nExec=\"/opt/ql/bin/quicklook-nemo\" %U\n",
        )
        .unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(!ours.exists());

        let user = applications.join("nemo.desktop");
        std::fs::write(&user, "[Desktop Entry]\nExec=nemo --user-custom %U\n").unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert_eq!(
            std::fs::read_to_string(&user).unwrap(),
            "[Desktop Entry]\nExec=nemo --user-custom %U\n"
        );

        // 早期版本生成的应用菜单项同样指向已删除的 quicklook-nemo，会被清掉。
        let legacy = applications.join("org.quicklook.Nemo.desktop");
        std::fs::write(
            &legacy,
            "[Desktop Entry]\nExec=\"/opt/ql/bin/quicklook-nemo\" %U\n",
        )
        .unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(!legacy.exists());
        // 名字相近但内容不带标记的条目不是我们的，不能误删。
        let user_legacy = applications.join("org.quicklook.Nemo.desktop");
        std::fs::write(&user_legacy, "[Desktop Entry]\nExec=nemo %U\n").unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        assert!(user_legacy.exists());
    }

    #[test]
    fn dolphin_service_menu_is_visible_for_every_file() {
        let (_root, prefix, data, config_home) = setup();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let menu =
            std::fs::read_to_string(data.join("kio/servicemenus/quicklook.desktop")).unwrap();
        // 没有 X-KDE-ServiceTypes 时 Dolphin 会忽略整个文件。
        assert!(menu.contains("X-KDE-ServiceTypes=KonqPopupMenu/Plugin"));
        assert!(menu.contains("all/allfiles"));
        assert!(menu.contains("inode/directory"));
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let (_root, prefix, data, config_home) = setup();
        let report = apply_at_with(&enabled(), &prefix, &data, &config_home, true).unwrap();
        assert!(
            report
                .written
                .iter()
                .any(|path| path.ends_with("quicklook.nemo_action"))
        );
        assert!(!data.join("nemo/actions/quicklook.nemo_action").exists());
        assert!(!launcher_path(&data, "thunar").exists());
        assert!(report.removed.is_empty());
    }

    #[test]
    fn removing_every_host_keeps_user_files() {
        let (_root, prefix, data, config_home) = setup();
        std::fs::create_dir_all(config_home.join("yazi")).unwrap();
        let user_init = "require(\"user-plugin\")\n";
        std::fs::write(config_home.join("yazi/init.lua"), user_init).unwrap();
        apply_at(&enabled(), &prefix, &data, &config_home).unwrap();
        let removed = Config {
            hijack_native: false,
            host_nemo: false,
            host_dolphin: false,
            host_nautilus: false,
            host_thunar: false,
            host_yazi: false,
            ..enabled()
        };
        apply_at(&removed, &prefix, &data, &config_home).unwrap();
        assert!(!data.join("nemo/actions/quicklook.nemo_action").exists());
        assert!(!data.join("kio/servicemenus/quicklook.desktop").exists());
        assert!(!data.join("nautilus/scripts/Quick Look").exists());
        assert!(!data.join("applications/thunar.desktop").exists());
        assert!(!launcher_path(&data, "thunar").exists());
        assert!(!config_home.join("yazi/plugins/quicklook.yazi").exists());
        // 用户自己写的 Lua 必须原样保留，托管块被移除。
        let init = std::fs::read_to_string(config_home.join("yazi/init.lua")).unwrap();
        assert_eq!(init.trim_end(), user_init.trim_end());
        // 图标属于应用资源，不随宿主开关移除。
        assert!(
            data.join("icons/hicolor/48x48/apps/org.quicklook.Preview.png")
                .exists()
        );
    }

    #[test]
    fn autostart_entry_points_at_the_broker() {
        let (_root, prefix, _data, config_home) = setup();
        let config = Config {
            autostart: true,
            ..Config::default()
        };
        let mut report = Report::default();
        apply_autostart(&config, &config_home, &prefix, &mut report).unwrap();
        let path = config_home.join("autostart/org.quicklook.Preview.desktop");
        assert_eq!(report.written, vec![path.clone()]);
        let text = std::fs::read_to_string(&path).unwrap();
        // 只启动常驻 broker：预览窗口按需拉起，不该在登录时占内存。
        assert!(text.contains("Exec=\""));
        assert!(text.contains("quicklookd"));
        assert!(!text.contains("quicklook-viewer"));
        assert!(text.contains("Icon=org.quicklook.Preview"));
        assert!(text.starts_with("[Desktop Entry]"));
    }
    #[test]
    fn autostart_second_apply_is_a_noop_and_disable_removes_it() {
        let (_root, prefix, _data, config_home) = setup();
        let on = Config {
            autostart: true,
            ..Config::default()
        };
        let mut report = Report::default();
        apply_autostart(&on, &config_home, &prefix, &mut report).unwrap();
        let mut again = Report::default();
        apply_autostart(&on, &config_home, &prefix, &mut again).unwrap();
        assert!(again.written.is_empty() && again.removed.is_empty());
        let path = config_home.join("autostart/org.quicklook.Preview.desktop");
        assert!(path.is_file());
        let mut off = Report::default();
        apply_autostart(&Config::default(), &config_home, &prefix, &mut off).unwrap();
        assert_eq!(off.removed, vec![path.clone()]);
        assert!(!path.exists());
        // 删掉条目时不该顺手把 autostart 目录下的别人条目带走。
        let mut empty = Report::default();
        apply_autostart(&Config::default(), &config_home, &prefix, &mut empty).unwrap();
        assert!(empty.removed.is_empty());
    }
    #[test]
    fn autostart_source_escapes_quotes_in_the_path() {
        // 路径里带引号时 desktop 条目仍要合法：desktop_exec_arg 会转义，外面再套引号。
        let entry = autostart_entry(Path::new("/opt/we\"ird/bin"));
        assert!(entry.contains("Exec=\""));
        assert!(!entry.contains("\nExec=\"\""));
    }

    /// niri-stack 的 Mod+E 脚本：必须在 deb 路径下也能被接管。
    fn shim_fixture(prefix: &Path, config_home: &Path) -> PathBuf {
        let home = config_home.parent().unwrap();
        let path = home.join(".niri-stack/prefix/bin/launch-file-manager");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "#!/bin/sh\n# 供 niri 键位调用\nexec env -u GTK_IM_MODULE GDK_BACKEND=wayland thunar \"$@\"\n",
        )
        .unwrap();
        let _ = prefix;
        path
    }

    fn launcher_fixture(data: &Path) -> PathBuf {
        let path = data.join("quicklook/launchers/quicklook-thunar");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        path
    }

    #[test]
    fn launcher_shim_gets_managed_block_and_stays_idempotent() {
        let (_root, prefix, data, config_home) = setup();
        let shim = shim_fixture(&prefix, &config_home);
        let launcher = launcher_fixture(&data);
        let config = Config {
            hijack_native: true,
            ..Config::default()
        };
        apply_at(&config, &prefix, &data, &config_home).unwrap();
        let text = std::fs::read_to_string(&shim).unwrap();
        assert!(text.contains(SHIM_BEGIN) && text.contains(SHIM_END));
        assert!(text.contains(&format!(
            "exec env -u GTK_IM_MODULE GDK_BACKEND=wayland {} \"$@\"",
            shell_quote(&launcher)
        )));
        // 原来那条命令必须留着当兜底。
        assert_eq!(text.matches(SHIM_COMMAND).count(), 1);
        // 再跑一次不该产生写入。
        let again = apply_at(&config, &prefix, &data, &config_home).unwrap();
        assert!(
            !again.written.contains(&shim),
            "第二次不应重写：{:?}",
            again.written
        );
        let text2 = std::fs::read_to_string(&shim).unwrap();
        assert_eq!(text, text2);
    }

    #[test]
    fn disabling_takeover_restores_the_original_shim() {
        let (_root, prefix, data, config_home) = setup();
        let shim = shim_fixture(&prefix, &config_home);
        let original = std::fs::read_to_string(&shim).unwrap();
        launcher_fixture(&data);
        let on = Config {
            hijack_native: true,
            ..Config::default()
        };
        apply_at(&on, &prefix, &data, &config_home).unwrap();
        assert!(std::fs::read_to_string(&shim).unwrap().contains(SHIM_BEGIN));
        let off = Config {
            hijack_native: false,
            ..Config::default()
        };
        apply_at(&off, &prefix, &data, &config_home).unwrap();
        assert_eq!(std::fs::read_to_string(&shim).unwrap(), original);
    }

    #[test]
    fn custom_shim_command_is_preserved_with_a_note() {
        let (_root, prefix, data, config_home) = setup();
        let home = config_home.parent().unwrap();
        let shim = home.join(".niri-stack/prefix/bin/launch-file-manager");
        std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
        let custom = "#!/bin/sh\nexec /usr/bin/thunar --display :1 \"$@\"\n";
        std::fs::write(&shim, custom).unwrap();
        launcher_fixture(&data);
        let on = Config {
            hijack_native: true,
            ..Config::default()
        };
        let report = apply_at(&on, &prefix, &data, &config_home).unwrap();
        assert_eq!(std::fs::read_to_string(&shim).unwrap(), custom);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("不是已知形式"))
        );
    }

    #[test]
    fn launcher_shim_is_left_alone_when_thunar_is_not_a_host() {
        let (_root, prefix, data, config_home) = setup();
        let shim = shim_fixture(&prefix, &config_home);
        let original = std::fs::read_to_string(&shim).unwrap();
        launcher_fixture(&data);
        let off = Config {
            hijack_native: true,
            host_thunar: false,
            ..Config::default()
        };
        let report = apply_at(&off, &prefix, &data, &config_home).unwrap();
        assert_eq!(std::fs::read_to_string(&shim).unwrap(), original);
        assert!(!report.written.contains(&shim));
    }
}
