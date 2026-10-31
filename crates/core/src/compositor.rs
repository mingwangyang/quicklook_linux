//! 合成器判定与「没有托管规则的桌面该怎么办」的提示。
//!
//! 程序本身不依赖任何合成器：GTK4 画窗口，D-Bus 传指令，X11 与各 Wayland
//! 桌面都能跑。唯一需要按桌面适配的是**窗口布局策略**——平铺合成器会把预览
//! 窗口当成普通窗口塞进布局，这就需要一条 `open-floating` 之类的规则。
//!
//! niri 的规则由 `niri` 模块托管（写文件并校验）。sway 与 Hyprland 的配置
//! 语法差异大、本机也没有会话可以验证，因此这里只做**识别并打印可直接粘贴
//! 的片段**，不擅自改它们的配置。GNOME / KDE / Cinnamon / MATE / Xfce 这类
//! 不自动平铺的桌面本来就把新窗口浮动显示并给焦点，开箱即用。

use std::path::Path;

/// 当前桌面是否需要用户自己加窗口规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    /// niri：规则由程序托管，不需要用户动手。
    Niri,
    /// sway：平铺合成器，需要 `for_window`。
    Sway,
    /// Hyprland：平铺合成器，需要 `windowrulev2`。
    Hyprland,
    /// 不自动平铺的桌面，默认就是浮动窗口。
    Floating,
}

impl Desktop {
    /// 需要用户手工加的配置片段；`None` 表示不用管。
    pub fn snippet(self) -> Option<&'static str> {
        match self {
            Desktop::Niri | Desktop::Floating => None,
            Desktop::Sway => Some(
                "# 加进 ~/.config/sway/config\n\
                 for_window [app_id=\"^org\\.quicklook\\.Preview$\"] floating enable\n\
                 for_window [app_id=\"^org\\.quicklook\\.Preview\\.Settings$\"] floating enable\n\
                 for_window [app_id=\"^org\\.quicklook\\.Preview\\.Detached$\"] floating disable",
            ),
            Desktop::Hyprland => Some(
                "# 加进 ~/.config/hypr/hyprland.conf\n\
                 windowrulev2 = float, class:^(org\\.quicklook\\.Preview)$\n\
                 windowrulev2 = float, class:^(org\\.quicklook\\.Preview\\.Settings)$",
            ),
        }
    }
}

/// 从配置目录与运行时环境变量判断当前桌面。
///
/// 先看是否存在对应合成器的配置，再看会话环境变量；都没有时按「不自动平铺」
/// 处理——那是最常见的情况，也不该吓唬用户。
pub fn detect(config_home: &Path) -> Desktop {
    if config_home.join("niri/config.kdl").is_file() {
        return Desktop::Niri;
    }
    if config_home.join("sway/config").is_file() || std::env::var_os("SWAYSOCK").is_some() {
        return Desktop::Sway;
    }
    if config_home.join("hypr/hyprland.conf").is_file()
        || std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
    {
        return Desktop::Hyprland;
    }
    Desktop::Floating
}

/// 给 `--apply-hosts` 的说明行：需要用户加规则时打印片段，否则一句话带过。
pub fn hint(config_home: &Path) -> Option<String> {
    match detect(config_home) {
        Desktop::Niri => None,
        Desktop::Sway => Some(format!(
            "检测到 sway：它是平铺合成器，需要这两行才会浮动（niri 的规则文件不适用）。\n{}",
            Desktop::Sway.snippet().unwrap_or_default()
        )),
        Desktop::Hyprland => Some(format!(
            "检测到 Hyprland：它是平铺合成器，需要这两行才会浮动（niri 的规则文件不适用）。\n{}",
            Desktop::Hyprland.snippet().unwrap_or_default()
        )),
        Desktop::Floating => {
            Some("当前桌面不自动平铺，预览窗口默认就是浮动的，无需额外窗口规则。".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niri_config_wins() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("niri")).unwrap();
        std::fs::write(home.path().join("niri/config.kdl"), "input {}\n").unwrap();
        assert_eq!(detect(home.path()), Desktop::Niri);
    }

    #[test]
    fn sway_and_hyprland_are_detected_from_config_files() {
        let sway = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(sway.path().join("sway")).unwrap();
        std::fs::write(sway.path().join("sway/config"), "# sway\n").unwrap();
        assert_eq!(detect(sway.path()), Desktop::Sway);

        let hypr = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(hypr.path().join("hypr")).unwrap();
        std::fs::write(hypr.path().join("hypr/hyprland.conf"), "# hypr\n").unwrap();
        assert_eq!(detect(hypr.path()), Desktop::Hyprland);
    }

    #[test]
    fn plain_desktop_needs_no_rule() {
        let home = tempfile::tempdir().unwrap();
        // 环境变量缺失时按不自动平铺处理（本机是 niri 会话，但配置目录里没有 niri 配置）。
        if std::env::var_os("SWAYSOCK").is_none()
            && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none()
        {
            assert_eq!(detect(home.path()), Desktop::Floating);
            assert_eq!(Desktop::Floating.snippet(), None);
            assert!(hint(home.path()).unwrap().contains("无需额外窗口规则"));
        }
    }

    #[test]
    fn snippets_are_paste_ready() {
        assert!(Desktop::Sway.snippet().unwrap().contains("for_window"));
        assert!(
            Desktop::Hyprland
                .snippet()
                .unwrap()
                .contains("windowrulev2 = float")
        );
        // niri 由程序托管，不给片段。
        assert_eq!(Desktop::Niri.snippet(), None);
    }
}
