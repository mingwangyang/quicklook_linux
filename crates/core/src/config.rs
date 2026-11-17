use crate::keys::PreviewKey;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub cache_mb: usize,
    pub image_max_edge: u32,
    pub idle_timeout_secs: u64,
    pub viewer_grace_secs: u64,
    pub text_chunk_bytes: usize,
    pub max_text_buffer_bytes: usize,
    pub max_archive_entries: usize,
    pub max_extracted_bytes: u64,
    pub nemo_compat: bool,
    /// 所有宿主统一的预览键；默认空格。
    pub preview_key: String,
    /// 单个宿主的覆盖按键；空字符串表示跟随 `preview_key`。
    /// Nautilus 与 Dolphin 之外都用同一个解析规则。
    pub preview_key_nemo: String,
    pub preview_key_thunar: String,
    pub preview_key_caja: String,
    pub preview_key_yazi: String,
    pub preview_key_nautilus: String,
    /// 预览界面主题：`light`（默认）或 `dark`。
    pub theme: String,
    /// 是否在 Markdown 里渲染 ```mermaid 流程图。
    pub mermaid: bool,
    /// 预览窗口默认布局：`floating`（默认）或 `tiled`。
    pub window_mode: String,
    /// 预览窗口默认宽度（逻辑像素）。0 表示按内容自动。
    pub window_width: u32,
    /// 预览窗口默认高度（逻辑像素）。0 表示按内容自动。
    pub window_height: u32,
    /// PDF 预览内容区背景色（#RRGGBB）。默认向常见 PDF 阅读器的深灰看齐。
    pub background_pdf: String,
    /// 图片预览内容区背景色（#RRGGBB）。默认向常见图片查看器的深灰看齐。
    pub background_image: String,
    /// 视频预览内容区背景色（#RRGGBB）。默认向 mpv / VLC 的纯黑看齐。
    pub background_video: String,
    /// 音频预览内容区背景色（#RRGGBB）。默认向 Audacity 的深灰看齐。
    pub background_audio: String,
    /// Word / PPT 等文档预览内容区背景色（#RRGGBB）。默认向 LibreOffice 的桌面灰看齐。
    pub background_office: String,
    /// Excel / CSV 等表格预览内容区背景色（#RRGGBB）。默认向 Calc 的单元格白看齐。
    pub background_sheet: String,
    /// 文本 / 代码 / Markdown / HTML 预览内容区背景色（#RRGGBB）。空表示跟随主题。
    pub background_text: String,
    /// 其它类型（目录、压缩包、字体等）的背景色（#RRGGBB）。空表示跟随主题。
    pub background_other: String,
    /// PDF 是否默认进入连续滚动模式。
    pub pdf_continuous: bool,
    /// 是否接管宿主自带的预览入口与同键原有行为。
    pub hijack_native: bool,
    /// 登录时自动启动常驻服务（通知栏图标 + 预热）。写 XDG autostart 条目。
    pub autostart: bool,
    pub host_nemo: bool,
    pub host_dolphin: bool,
    pub host_nautilus: bool,
    pub host_thunar: bool,
    pub host_caja: bool,
    pub host_yazi: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            cache_mb: 200,
            image_max_edge: 2560,
            idle_timeout_secs: 600,
            viewer_grace_secs: 3,
            text_chunk_bytes: 256 * 1024,
            max_text_buffer_bytes: 8 * 1024 * 1024,
            max_archive_entries: 10_000,
            max_extracted_bytes: 128 * 1024 * 1024,
            nemo_compat: false,
            preview_key: "Space".into(),
            preview_key_nemo: String::new(),
            preview_key_thunar: String::new(),
            preview_key_caja: String::new(),
            preview_key_yazi: String::new(),
            preview_key_nautilus: String::new(),
            theme: "light".into(),
            mermaid: true,
            window_mode: "floating".into(),
            window_width: 940,
            window_height: 650,
            background_pdf: "#333333".into(),
            background_image: "#1f1f1f".into(),
            background_video: "#000000".into(),
            background_audio: "#1e1e1e".into(),
            background_office: "#e6e6e6".into(),
            background_sheet: "#ffffff".into(),
            background_text: String::new(),
            background_other: String::new(),
            pdf_continuous: true,
            hijack_native: false,
            // 默认不开：装完不会悄悄往 autostart 里塞条目，用户要么在设置里打开，
            // 要么第一次预览时让 broker 自己起来（之后一直驻留）。
            autostart: false,
            host_nemo: true,
            host_dolphin: true,
            host_nautilus: true,
            host_thunar: true,
            host_caja: true,
            host_yazi: true,
        }
    }
}
pub fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
}
pub fn xdg_data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
}
pub fn config_dir() -> PathBuf {
    xdg_config_home().join("quicklook")
}
pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        })
        .join("quicklook")
}
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}
/// 校验 `#RRGGBB`；非法值返回空字符串（交给使用方用默认色）。
fn normalize_color(value: &str) -> String {
    let text = value.trim();
    let bytes = text.as_bytes();
    if bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
    {
        text.to_ascii_lowercase()
    } else {
        String::new()
    }
}
impl Config {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&config_path())
    }
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut value: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        value.normalize();
        Ok(value)
    }
    pub fn save(&self) -> anyhow::Result<PathBuf> {
        self.save_to(&config_path())
    }
    pub fn save_to(&self, path: &Path) -> anyhow::Result<PathBuf> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut value = self.clone();
        value.normalize();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write;
        temporary.write_all(toml::to_string_pretty(&value)?.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(path)?;
        Ok(path.to_path_buf())
    }
    /// 统一预览键；无法解析时退回空格，避免坏配置让快捷键凭空消失。
    pub fn key(&self) -> PreviewKey {
        PreviewKey::parse(&self.preview_key).unwrap_or_default()
    }
    /// 某个宿主的实际按键：没有覆盖（或覆盖写坏了）就跟随全局默认。
    pub fn key_for(&self, override_key: &str) -> PreviewKey {
        if override_key.trim().is_empty() {
            return self.key();
        }
        PreviewKey::parse(override_key).unwrap_or_else(|| self.key())
    }
    pub fn nemo_key(&self) -> PreviewKey {
        self.key_for(&self.preview_key_nemo)
    }
    pub fn thunar_key(&self) -> PreviewKey {
        self.key_for(&self.preview_key_thunar)
    }
    pub fn caja_key(&self) -> PreviewKey {
        self.key_for(&self.preview_key_caja)
    }
    pub fn yazi_key(&self) -> PreviewKey {
        self.key_for(&self.preview_key_yazi)
    }
    pub fn nautilus_key(&self) -> PreviewKey {
        self.key_for(&self.preview_key_nautilus)
    }
    /// 深浅主题；未知值退回浅色，保证界面不会因手改配置变成随机配色。
    pub fn uses_dark_theme(&self) -> bool {
        self.theme.eq_ignore_ascii_case("dark")
    }
    /// 用户设置的窗口默认尺寸；两个都为 0 时返回 None，表示按屏幕自动。
    pub fn window_size(&self) -> Option<(i32, i32)> {
        (self.window_width != 0 && self.window_height != 0)
            .then_some((self.window_width as i32, self.window_height as i32))
    }
    /// 预览窗口是否浮动；未知值退回浮动，默认行为不能因手改配置变成平铺。
    pub fn uses_floating_layout(&self) -> bool {
        !self.window_mode.eq_ignore_ascii_case("tiled")
            && !self.window_mode.eq_ignore_ascii_case("tile")
    }
    fn normalize(&mut self) {
        self.theme = if self.uses_dark_theme() {
            "dark"
        } else {
            "light"
        }
        .into();
        self.window_mode = if self.uses_floating_layout() {
            "floating"
        } else {
            "tiled"
        }
        .into();
        self.cache_mb = self.cache_mb.clamp(8, 2048);
        self.viewer_grace_secs = self.viewer_grace_secs.min(30);
        self.image_max_edge = self.image_max_edge.clamp(256, 8192);
        // 0 表示自动；其余夹到合理范围，避免手改配置把窗口写成 0x0 或超出屏幕。
        if self.window_width != 0 {
            self.window_width = self.window_width.clamp(480, 7680);
        }
        if self.window_height != 0 {
            self.window_height = self.window_height.clamp(320, 4320);
        }
        for color in [
            &mut self.background_pdf,
            &mut self.background_image,
            &mut self.background_video,
            &mut self.background_audio,
            &mut self.background_office,
            &mut self.background_sheet,
            &mut self.background_text,
            &mut self.background_other,
        ] {
            *color = normalize_color(color);
        }
        self.text_chunk_bytes = self.text_chunk_bytes.clamp(4096, 1024 * 1024);
        self.max_text_buffer_bytes = self
            .max_text_buffer_bytes
            .clamp(1024 * 1024, 64 * 1024 * 1024);
        self.max_archive_entries = self.max_archive_entries.clamp(100, 100_000);
        self.max_extracted_bytes = self
            .max_extracted_bytes
            .clamp(1024 * 1024, 1024 * 1024 * 1024);
        self.preview_key = self.key().to_string();
        // 覆盖键：留空表示跟随全局；写坏的值退回跟随，而不是替用户猜一个键。
        for field in [
            &mut self.preview_key_nemo,
            &mut self.preview_key_thunar,
            &mut self.preview_key_caja,
            &mut self.preview_key_yazi,
            &mut self.preview_key_nautilus,
        ] {
            if !field.trim().is_empty() {
                match PreviewKey::parse(field) {
                    Some(key) => *field = key.to_string(),
                    None => field.clear(),
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_size_and_pdf_defaults() {
        let config = Config::default();
        assert_eq!(config.window_size(), Some((940, 650)));
        assert!(config.pdf_continuous);
        let mut fixed = config.clone();
        fixed.window_width = 1120;
        fixed.window_height = 790;
        assert_eq!(fixed.window_size(), Some((1120, 790)));
        let mut auto = config.clone();
        auto.window_width = 0;
        auto.window_height = 0;
        assert_eq!(auto.window_size(), None);
        assert_eq!(config.background_pdf, "#333333");
        assert_eq!(config.background_image, "#1f1f1f");
        assert_eq!(config.background_video, "#000000");
        assert_eq!(config.background_audio, "#1e1e1e");
        assert_eq!(config.background_office, "#e6e6e6");
        assert_eq!(config.background_sheet, "#ffffff");
        assert!(config.background_text.is_empty());
    }

    #[test]
    fn defaults_use_space_and_all_hosts() {
        let config = Config::default();
        assert_eq!(config.preview_key, "Space");
        assert_eq!(config.theme, "light");
        assert!(!config.uses_dark_theme());
        assert!(!config.hijack_native);
        assert!(
            config.host_nemo
                && config.host_dolphin
                && config.host_nautilus
                && config.host_caja
                && config.host_yazi
        );
    }
    #[test]
    fn theme_accepts_dark_and_falls_back_to_light() {
        let dark = Config {
            theme: "Dark".into(),
            ..Config::default()
        };
        assert!(dark.uses_dark_theme());
        let broken = Config {
            theme: "solarized".into(),
            ..Config::default()
        };
        assert!(!broken.uses_dark_theme());
        let path = tempfile::tempdir().unwrap().path().join("config.toml");
        broken.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().theme, "light");
    }
    #[test]
    fn host_keys_follow_the_global_default_unless_overridden() {
        let config = Config {
            preview_key: "Ctrl+Space".into(),
            preview_key_nemo: "f3".into(),
            preview_key_caja: "alt+f5".into(),
            preview_key_yazi: "Hyper+Q".into(),
            ..Config::default()
        };
        assert_eq!(config.nemo_key().to_string(), "F3");
        assert_eq!(config.caja_key().to_string(), "Alt+F5");
        // 非法覆盖退回跟随全局，而不是凭空变成一个具体按键。
        assert_eq!(config.yazi_key().to_string(), "Ctrl+Space");
        assert_eq!(config.thunar_key().to_string(), "Ctrl+Space");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        config.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.preview_key_nemo, "F3");
        assert_eq!(loaded.preview_key_caja, "Alt+F5");
        assert_eq!(loaded.preview_key_yazi, "");
        assert_eq!(loaded.preview_key, "Ctrl+Space");
        // 默认配置里所有宿主都跟随全局，不写任何覆盖。
        let plain = Config::default();
        assert_eq!(plain.preview_key_nemo, "");
        assert_eq!(plain.nemo_key(), plain.key());
        assert_eq!(plain.nautilus_key(), plain.key());
    }
    #[test]
    fn window_mode_defaults_to_floating_and_normalizes() {
        let config = Config::default();
        assert_eq!(config.window_mode, "floating");
        assert!(config.uses_floating_layout());
        let tiled = Config {
            window_mode: "Tiled".into(),
            ..Config::default()
        };
        assert!(!tiled.uses_floating_layout());
        let broken = Config {
            window_mode: "grid".into(),
            ..Config::default()
        };
        assert!(broken.uses_floating_layout());
        let path = tempfile::tempdir().unwrap().path().join("config.toml");
        broken.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().window_mode, "floating");
        tiled.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().window_mode, "tiled");
    }
    #[test]
    fn unknown_fields_still_fail_loudly() {
        let error = toml::from_str::<Config>("cache_mb = 10\ntypo_key = 1\n").unwrap_err();
        assert!(error.to_string().contains("typo_key"));
    }
    #[test]
    fn invalid_key_falls_back_to_space() {
        let config = Config {
            preview_key: "Hyper+Q".into(),
            ..Config::default()
        };
        assert_eq!(config.key().to_string(), "Space");
    }
    #[test]
    fn round_trips_through_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let config = Config {
            preview_key: "ctrl+shift+f3".into(),
            host_yazi: false,
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.preview_key, "Ctrl+Shift+F3");
        assert!(!loaded.host_yazi);
        assert_eq!(loaded.cache_mb, config.cache_mb);
    }
}
