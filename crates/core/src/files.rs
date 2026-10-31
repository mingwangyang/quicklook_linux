use anyhow::{Context, Result, bail};
use gio::prelude::*;
use serde::Serialize;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Image,
    Media,
    Pdf,
    Text,
    Markdown,
    Directory,
    Archive,
    Binary,
    Html,
    Structured,
    Rtf,
    Font,
    Eps,
    Raw,
    Psd,
    Office,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileInfo {
    pub uri: String,
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub modified_ns: u128,
    pub kind: Kind,
}
fn serialize_path<S: serde::Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}
impl FileInfo {
    pub fn from_arg(arg: impl AsRef<OsStr>) -> Result<Self> {
        let file = gio::File::for_commandline_arg(arg.as_ref());
        let path = file
            .path()
            .context("仅支持本机可访问的文件；远端文件请先挂载到本地")?;
        Self::from_path(&path)
    }
    pub fn from_path(path: &Path) -> Result<Self> {
        let path =
            std::fs::canonicalize(path).with_context(|| format!("无法访问 {}", path.display()))?;
        let metadata = path.metadata()?;
        if !metadata.is_file() && !metadata.is_dir() {
            bail!("不预览设备、管道或套接字");
        }
        let uri = gio::File::for_path(&path).uri().to_string();
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let (content_type, _) = gio::content_type_guess(Some(&path), None::<&[u8]>);
        let mime = gio::content_type_get_mime_type(&content_type)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".into());
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let kind = if metadata.is_dir() {
            Kind::Directory
        } else {
            classify(&ext, &mime)
        };
        // 系统装了 WPS 时，shared-mime-info 会把 docx 等类型别名成 `application/wps-office.docx`，
        // 直接显示在标题上会让人误以为是 WPS 专有格式；这里对 Office 家族用规范 MIME。
        let mime = if kind == Kind::Office {
            office_mime(&ext).to_string()
        } else {
            mime
        };
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|t| t.as_nanos())
            .unwrap_or(0);
        Ok(Self {
            uri,
            path,
            name,
            mime,
            size: metadata.len(),
            modified_ns,
            kind,
        })
    }
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.uri, self.size, self.modified_ns)
    }
    pub fn title(&self) -> String {
        // Office 的规范 MIME 有一百多个字符，直接放到标题栏会被截得难看；
        // 用「Word 文档」这类短标签，完整 MIME 仍然能在属性面板里看到。
        let label = match self.kind {
            Kind::Office => office_label(&self.path),
            // CSV 直接说「CSV 表格」比 text/csv 好读；标题栏要短。
            Kind::Text if self.mime == "text/csv" => "CSV 表格".to_string(),
            Kind::Text if self.mime == "text/tab-separated-values" => "TSV 表格".to_string(),
            _ => self.mime.clone(),
        };
        format!("{} · {}", label, format_size(self.size))
    }
}
/// 标题栏用的短标签。
fn office_label(path: &Path) -> String {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    match ext.as_str() {
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" => "Excel 表格",
        "pptx" | "pptm" | "potx" | "ppsx" | "ppsm" => "PowerPoint 演示文稿",
        _ => "Word 文档",
    }
    .to_string()
}
/// OOXML 家族的规范 MIME，避免依赖系统里可能被第三方套件改写的别名。
fn office_mime(ext: &str) -> &'static str {
    const WORD: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    const SHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
    const SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
    match ext {
        "docx" | "docm" | "dotx" | "dotm" => WORD,
        "xlsx" | "xlsm" | "xltx" | "xltm" => SHEET,
        "pptx" | "pptm" | "potx" | "ppsx" | "ppsm" => SLIDE,
        _ => "application/octet-stream",
    }
}
pub fn classify(ext: &str, mime: &str) -> Kind {
    match ext {
        "pdf" | "ai" => Kind::Pdf,
        "eps" => Kind::Eps,
        "psd" => Kind::Psd,
        "cr2" | "cr3" | "nef" | "arw" | "dng" | "raf" | "orf" | "rw2" => Kind::Raw,
        "md" | "markdown" | "mdown" => Kind::Markdown,
        "html" | "htm" | "xhtml" => Kind::Html,
        "json" | "jsonc" | "xml" | "yaml" | "yml" => Kind::Structured,
        "rtf" => Kind::Rtf,
        "ttf" | "otf" | "woff" | "woff2" | "ttc" => Kind::Font,
        "docx" | "docm" | "dotx" | "dotm" | "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "pptx"
        | "pptm" | "potx" | "ppsx" | "ppsm" => Kind::Office,
        "zip" | "tar" | "gz" | "tgz" | "xz" | "txz" | "bz2" | "7z" | "rar" | "zst" => Kind::Archive,
        "heic" | "heif" | "avif" | "jxl" | "tif" | "tiff" => Kind::Image,
        _ if mime.starts_with("image/") => Kind::Image,
        _ if mime.starts_with("video/") || mime.starts_with("audio/") => Kind::Media,
        _ if mime.starts_with("text/")
            || matches!(
                ext,
                "toml"
                    | "ini"
                    | "conf"
                    | "config"
                    | "log"
                    | "rs"
                    | "py"
                    | "sh"
                    | "c"
                    | "h"
                    | "cpp"
                    | "go"
                    | "js"
                    | "ts"
                    | "jsx"
                    | "tsx"
                    | "vue"
                    | "svelte"
                    | "css"
                    | "sql"
                    | "kdl"
                    | "nix"
            ) =>
        {
            Kind::Text
        }
        _ => Kind::Binary,
    }
}
pub fn siblings(current: &FileInfo) -> Vec<FileInfo> {
    let Some(parent) = current.path.parent() else {
        return vec![current.clone()];
    };
    let show_hidden = current.name.starts_with('.');
    let mut files: Vec<_> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .take(20_000)
        .filter(|e| show_hidden || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| FileInfo::from_path(&e.path()).ok())
        .collect();
    files.sort_by(|a, b| natord::compare(&a.name.to_lowercase(), &b.name.to_lowercase()));
    files
}
pub fn format_size(size: u64) -> String {
    let mut n = size as f64;
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut i = 0;
    while n >= 1024.0 && i < units.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{size} B")
    } else {
        format!("{n:.1} {}", units[i])
    }
}
pub fn safe_archive_path(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && !name.contains('\\')
        && !name.contains('\0')
        && !path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
        && !name.split('/').any(|p| p.contains(':'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_archive_escape() {
        for s in ["../x", "/x", "a/../../x", "C:/x", "a\\..\\x"] {
            assert!(!safe_archive_path(s), "{s}");
        }
        assert!(safe_archive_path("目录/file.txt"));
    }
    #[test]
    fn filenames_are_passed_as_uris() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("中文 a\n#?.txt");
        std::fs::write(&path, "hello").unwrap();
        let info = FileInfo::from_path(&path).unwrap();
        assert_eq!(gio::File::for_uri(&info.uri).path().unwrap(), path);
    }
    #[test]
    fn non_utf8_filename_status_is_serializable() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(std::ffi::OsString::from_vec(b"file-\xff.txt".to_vec()));
        std::fs::write(&path, "hello").unwrap();
        let info = FileInfo::from_path(&path).unwrap();
        assert_eq!(gio::File::for_uri(&info.uri).path().unwrap(), path);
        assert!(serde_json::to_string(&info).is_ok());
    }
    #[test]
    fn numeric_navigation() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["10.png", "2.png", "1.png", ".hidden"] {
            std::fs::write(dir.path().join(name), []).unwrap();
        }
        let items = siblings(&FileInfo::from_path(&dir.path().join("1.png")).unwrap());
        assert_eq!(
            items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["1.png", "2.png", "10.png"]
        );
    }
}
