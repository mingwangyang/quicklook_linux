//! niri 窗口规则的受管片段。
//!
//! 客户端不能在 Wayland 上自己要求浮动，预览窗口的默认布局由 niri 的
//! `open-floating` 规则决定。安装器写下的 `quicklook.kdl` 是受管文件：
//! 设置里改布局时整段规则重写，文件尾部的 `binds`（安装器按需追加）保持不变。

use crate::config::xdg_config_home;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 把规则渲染成 niri 配置；保留调用者原有的 `binds` 段。
pub fn render(existing: &str, floating: bool) -> String {
    let rules = template(floating);
    match existing.find("\nbinds {") {
        Some(at) => format!("{rules}\n{}", &existing[at + 1..]),
        None => rules,
    }
}

fn template(floating: bool) -> String {
    let dock = if floating { "true" } else { "false" };
    format!(
        r#"// 文件选择器是独立对话框：默认平铺会被塞进当前列，应该浮在中间并接收焦点。
// GTK4 的 FileDialog 走 xdg-desktop-portal-gtk，窗口的 app-id 不是我们的应用。
window-rule {{
    match app-id="^xdg-desktop-portal-gtk$" title="^选择要预览的文件$"
    open-floating true
}}

window-rule {{
    match app-id="^org\\.quicklook\\.Preview$" title="^选择要预览的文件$"
    open-floating true
    geometry-corner-radius 12
    clip-to-geometry true
}}

window-rule {{
    match app-id="^org\\.quicklook\\.Preview$"
    exclude title="^选择要预览的文件$"
    open-floating {dock}
    // 预览要接收随后的 Space/Esc；不给焦点的话按键会落回文件管理器，
    // 下一次空格又触发一次切换，看起来就是窗口闪一下立刻消失。
    open-focused true
    geometry-corner-radius 12
    clip-to-geometry true
}}

// 独立窗口按普通窗口平铺，仍然不抢焦点。
window-rule {{
    match app-id="^org\\.quicklook\\.Preview\\.Detached$"
    open-floating false
    open-focused false
}}

// 设置是对话框：浮动即可，尺寸由应用自己请求。
window-rule {{
    match app-id="^org\\.quicklook\\.Preview\\.Settings$"
    open-floating true
}}
"#
    )
}

/// 更新 `config_home/niri/quicklook.kdl`；没有安装 niri 集成时什么也不做。
pub fn sync_at(config_home: &Path, floating: bool) -> Result<Option<PathBuf>> {
    let path = config_home.join("niri/quicklook.kdl");
    if !path.is_file() {
        return Ok(None);
    }
    let old =
        std::fs::read_to_string(&path).with_context(|| format!("无法读取 {}", path.display()))?;
    let new = render(&old, floating);
    if new != old {
        let temporary = path.with_extension("kdl.quicklook-tmp");
        std::fs::write(&temporary, &new)
            .with_context(|| format!("无法写入 {}", temporary.display()))?;
        if let Ok(metadata) = std::fs::metadata(&path) {
            let _ = std::fs::set_permissions(&temporary, metadata.permissions());
        }
        std::fs::rename(&temporary, &path)
            .with_context(|| format!("无法替换 {}", path.display()))?;
    }
    Ok(Some(path))
}

/// 用当前 XDG 目录同步规则。
pub fn sync(floating: bool) -> Result<Option<PathBuf>> {
    sync_at(&xdg_config_home(), floating)
}

/// 让 niri 重新读取配置；niri 没运行或不是 niri 会话时返回 false。
pub fn reload() -> bool {
    Command::new("niri")
        .args(["msg", "action", "load-config-file"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 在 `config.kdl` 里补一行 include；已经有就原样返回。
pub fn ensure_include(existing: &str) -> String {
    if has_include(existing) {
        return existing.to_string();
    }
    format!(
        "{}\n\n// Quick Look file preview\ninclude \"quicklook.kdl\"\n",
        existing.trim_end()
    )
}

/// 去掉受管的那行 include 和它上面的说明注释。
pub fn strip_include(existing: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut dropped = false;
    for line in existing.lines() {
        if line.trim().starts_with("include") && line.contains("quicklook.kdl") {
            // 顺带吞掉紧邻的说明注释与空行，避免留下尾巴。
            while matches!(lines.last(), Some(last) if last.trim() == "// Quick Look file preview" || last.trim().is_empty())
            {
                lines.pop();
            }
            dropped = true;
            continue;
        }
        lines.push(line);
    }
    if !dropped {
        return existing.to_string();
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn has_include(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim().starts_with("include") && line.contains("quicklook.kdl"))
}

/// 给规则文件补一个调出选文件窗口的按键；已有 `Ctrl+Alt+P` 就不动。
fn with_picker_binding(rules: &str, config: &str, cli: &Path) -> String {
    if config.contains("Ctrl+Alt+P") || rules.contains("Ctrl+Alt+P") {
        return rules.to_string();
    }
    format!(
        "{rules}\nbinds {{\n    Ctrl+Alt+P hotkey-overlay-title=\"文件预览\" {{ spawn {} \"--pick\"; }}\n}}\n",
        json_string(&cli.to_string_lossy())
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// 装好 niri 集成：写规则文件，并保证主配置 include 了它。
///
/// `deb` 安装不碰用户配置，预览窗口的浮动/接收焦点全靠这些规则，所以由
/// 「应用文件管理器接入」统一维护。返回规则文件路径与是否真的写了内容；
/// 没有 niri 配置时返回 `Ok(None)`。
pub fn install_at(
    config_home: &Path,
    floating: bool,
    cli: &Path,
    dry_run: bool,
) -> Result<Option<(PathBuf, bool)>> {
    let main = config_home.join("niri/config.kdl");
    if !main.is_file() {
        return Ok(None);
    }
    let rules_path = config_home.join("niri/quicklook.kdl");
    let original_main =
        std::fs::read_to_string(&main).with_context(|| format!("无法读取 {}", main.display()))?;
    let original_rules = std::fs::read_to_string(&rules_path).ok();
    let rules = with_picker_binding(
        &render(original_rules.as_deref().unwrap_or(""), floating),
        &original_main,
        cli,
    );
    let proposed = ensure_include(&original_main);
    let changed = original_rules.as_deref() != Some(rules.as_str()) || proposed != original_main;
    if dry_run || !changed {
        return Ok(Some((rules_path, changed)));
    }
    write(&rules_path, &rules)?;
    write(&main, &proposed)?;
    if let Err(error) = validate(&main) {
        // 校验不过就整体回滚，不留半套规则。
        restore(&rules_path, original_rules.as_deref())?;
        restore(&main, Some(&original_main))?;
        return Err(error);
    }
    Ok(Some((rules_path, true)))
}

/// 移除 niri 集成：删规则文件并从主配置摘掉 include。
pub fn remove_at(config_home: &Path, dry_run: bool) -> Result<Option<PathBuf>> {
    let main = config_home.join("niri/config.kdl");
    let rules_path = config_home.join("niri/quicklook.kdl");
    let existed = rules_path.is_file();
    let stripped = main
        .is_file()
        .then(|| std::fs::read_to_string(&main).ok())
        .flatten()
        .map(|text| strip_include(&text));
    if dry_run {
        return Ok(existed.then_some(rules_path));
    }
    if existed {
        std::fs::remove_file(&rules_path)
            .with_context(|| format!("无法删除 {}", rules_path.display()))?;
    }
    if let Some(text) = stripped {
        let before = std::fs::read_to_string(&main).unwrap_or_default();
        if text != before {
            write(&main, &text)?;
        }
    }
    Ok(existed.then_some(rules_path))
}

fn write(path: &Path, text: &str) -> Result<()> {
    let temporary = path.with_extension("kdl.quicklook-tmp");
    std::fs::write(&temporary, text)
        .with_context(|| format!("无法写入 {}", temporary.display()))?;
    if let Ok(metadata) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&temporary, metadata.permissions());
    }
    std::fs::rename(&temporary, path).with_context(|| format!("无法替换 {}", path.display()))?;
    Ok(())
}

fn restore(path: &Path, original: Option<&str>) -> Result<()> {
    match original {
        Some(text) => write(path, text),
        None => {
            let _ = std::fs::remove_file(path);
            Ok(())
        }
    }
}

fn validate(config: &Path) -> Result<()> {
    let output = match Command::new("niri")
        .args(["validate", "--config"])
        .arg(config)
        .output()
    {
        Ok(output) => output,
        // 没装 niri（或不在 niri 会话）就没法校验，按可用处理。
        Err(_) => return Ok(()),
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::bail!(
        "niri 拒绝新的窗口规则，已回滚：{}",
        stderr.trim().to_string() + stdout.trim()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_template_matches_installed_integration() {
        assert_eq!(
            render("", true),
            include_str!("../../../integration/niri.kdl")
        );
    }

    #[test]
    fn tiled_only_changes_the_preview_rule() {
        let docked = render("", false);
        assert!(docked.contains("open-floating false\n    // 预览要接收"));
        // 选择器、设置与独立窗口的行为不随默认布局改变。
        assert_eq!(docked.matches("open-floating true").count(), 3);
        assert_eq!(docked.matches("open-floating false").count(), 2);
    }

    #[test]
    fn ensure_include_adds_once() {
        let config = "input {}\n\nbinds {\n    Mod+E { spawn \"thunar\"; }\n}\n";
        let once = ensure_include(config);
        assert!(once.contains("include \"quicklook.kdl\""));
        assert!(once.contains("// Quick Look file preview"));
        // 幂等：再跑一次不变。
        assert_eq!(ensure_include(&once), once);
        // 原有内容一字不动。
        assert!(once.starts_with("input {}"));
        assert!(once.contains("Mod+E"));
    }

    #[test]
    fn strip_include_removes_comment_and_blank_line() {
        let config = "input {}\n\n// Quick Look file preview\ninclude \"quicklook.kdl\"\n";
        assert_eq!(strip_include(config), "input {}\n");
        // 没有 include 时原样返回。
        assert_eq!(strip_include("input {}\n"), "input {}\n");
    }

    #[test]
    fn picker_binding_only_when_absent() {
        let cli = Path::new("/usr/bin/quicklook");
        let rules = render("", true);
        let bound = with_picker_binding(&rules, "input {}", cli);
        assert!(bound.contains("Ctrl+Alt+P"));
        assert!(bound.contains("\"/usr/bin/quicklook\""));
        // 用户已经自己绑了 Ctrl+Alt+P 就不再塞。
        assert_eq!(
            with_picker_binding(&rules, "binds {\n    Ctrl+Alt+P { spawn \"x\"; }\n}", cli),
            rules
        );
        // 已有绑定段重复执行不再翻倍。
        assert_eq!(with_picker_binding(&bound, "input {}", cli), bound);
    }

    #[test]
    fn install_creates_missing_integration_and_remove_cleans_up() {
        let home = tempfile::tempdir().unwrap();
        let niri = home.path().join("niri");
        std::fs::create_dir_all(&niri).unwrap();
        // 没有 config.kdl 时不该凭空造文件。
        assert_eq!(
            install_at(home.path(), true, Path::new("/usr/bin/quicklook"), false).unwrap(),
            None
        );
        let main = niri.join("config.kdl");
        std::fs::write(&main, "input {}\n").unwrap();
        let (rules, changed) =
            install_at(home.path(), true, Path::new("/usr/bin/quicklook"), false)
                .unwrap()
                .unwrap();
        assert!(changed);
        assert_eq!(rules, niri.join("quicklook.kdl"));
        let text = std::fs::read_to_string(&rules).unwrap();
        assert!(text.contains("open-floating true"));
        assert!(text.contains("Ctrl+Alt+P"));
        let patched = std::fs::read_to_string(&main).unwrap();
        assert!(patched.contains("include \"quicklook.kdl\""));
        // 第二次调用没有改动。
        let (_, changed) = install_at(home.path(), true, Path::new("/usr/bin/quicklook"), false)
            .unwrap()
            .unwrap();
        assert!(!changed);
        // 切到平铺后规则跟着变。
        install_at(home.path(), false, Path::new("/usr/bin/quicklook"), false).unwrap();
        assert!(
            std::fs::read_to_string(&rules)
                .unwrap()
                .contains("open-floating false")
        );
        // 移除后文件消失、include 摘干净。
        assert_eq!(remove_at(home.path(), false).unwrap(), Some(rules.clone()));
        assert!(!rules.exists());
        assert!(
            !std::fs::read_to_string(&main)
                .unwrap()
                .contains("quicklook.kdl")
        );
    }

    #[test]
    fn install_dry_run_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let niri = home.path().join("niri");
        std::fs::create_dir_all(&niri).unwrap();
        let main = niri.join("config.kdl");
        std::fs::write(&main, "input {}\n").unwrap();
        let (_, changed) = install_at(home.path(), true, Path::new("/usr/bin/quicklook"), true)
            .unwrap()
            .unwrap();
        assert!(changed);
        assert!(!niri.join("quicklook.kdl").exists());
        assert!(!std::fs::read_to_string(&main).unwrap().contains("include"));
    }

    #[test]
    fn sync_preserves_binds_and_is_idempotent() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("niri/quicklook.kdl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let installed = format!(
            "{}\nbinds {{\n    Ctrl+Alt+P {{ spawn \"quicklook\" \"--pick\"; }}\n}}\n",
            render("", true)
        );
        std::fs::write(&path, &installed).unwrap();
        assert_eq!(sync_at(home.path(), false).unwrap(), Some(path.clone()));
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("Ctrl+Alt+P"));
        assert!(updated.contains("open-floating false\n    // 预览要接收"));
        sync_at(home.path(), false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), updated);
        // 没安装规则文件时不应凭空创建。
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(sync_at(empty.path(), true).unwrap(), None);
        assert!(!empty.path().join("niri/quicklook.kdl").exists());
    }
}
