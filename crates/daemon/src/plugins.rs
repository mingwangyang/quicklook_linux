//! Versioned helper manifests keep external previewers outside the GTK process.
use crate::backend::{Body, ImageData, Spec};
use anyhow::{Context, Result, bail, ensure};
use quicklook_core::{
    config::{Config, config_dir},
    files::FileInfo,
    text::TextChunk,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
struct Manifest {
    api: u32,
    id: String,
    mime_types: Vec<String>,
    extensions: Vec<String>,
    priority: i32,
    command: Vec<String>,
    output: String,
    timeout_ms: u64,
    enabled: bool,
    #[serde(skip)]
    directory: PathBuf,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            api: 1,
            id: String::new(),
            mime_types: Vec::new(),
            extensions: Vec::new(),
            priority: 0,
            command: Vec::new(),
            output: "text".into(),
            timeout_ms: 5000,
            enabled: true,
            directory: PathBuf::new(),
        }
    }
}
impl Manifest {
    fn validate(&self) -> Result<()> {
        ensure!(self.api == 1, "不支持此插件协议版本");
        ensure!(!self.id.is_empty() && self.id.len() <= 80, "无效插件 ID");
        ensure!(
            !self.command.is_empty() && self.command.len() <= 32,
            "插件必须指定命令数组"
        );
        ensure!(
            matches!(self.output.as_str(), "text" | "html" | "image" | "pdf"),
            "无效插件输出格式"
        );
        ensure!(self.command.iter().all(|s| s.len() <= 4096), "插件参数过长");
        ensure!(
            !self.mime_types.is_empty() || !self.extensions.is_empty(),
            "插件未声明格式"
        );
        Ok(())
    }
    fn score(&self, file: &FileInfo) -> i32 {
        let extension = file.path.extension().unwrap_or_default().to_string_lossy();
        let exact_extension = self
            .extensions
            .iter()
            .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&extension));
        let exact_mime = self.mime_types.iter().any(|m| m == &file.mime);
        let wildcard = self.mime_types.iter().any(|m| {
            m.strip_suffix("/*")
                .map(|prefix| file.mime.starts_with(&format!("{prefix}/")))
                .unwrap_or(false)
        });
        let base = if exact_extension {
            1000
        } else if exact_mime {
            500
        } else if wildcard {
            100
        } else {
            0
        };
        if base == 0 {
            0
        } else {
            base + self.priority.clamp(-50, 50)
        }
    }
}
static REGISTRY: OnceLock<Vec<Manifest>> = OnceLock::new();
fn manifests() -> &'static Vec<Manifest> {
    REGISTRY.get_or_init(|| {
        let mut by_id = HashMap::new();
        for directory in [
            PathBuf::from("/usr/share/quicklook/plugins"),
            config_dir().join("plugins"),
        ] {
            let mut paths: Vec<_> = std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
                .take(64)
                .collect();
            paths.sort();
            for path in paths {
                let parsed = (|| -> Result<Manifest> {
                    ensure!(path.metadata()?.len() <= 65536, "插件声明超过 64 KiB");
                    let mut manifest: Manifest = toml::from_str(&std::fs::read_to_string(&path)?)?;
                    manifest.validate()?;
                    manifest.directory = path.parent().unwrap_or(Path::new("/")).to_path_buf();
                    Ok(manifest)
                })();
                match parsed {
                    Ok(manifest) => {
                        by_id.insert(manifest.id.clone(), manifest);
                    }
                    Err(e) => eprintln!("插件 {}：{e}", path.display()),
                }
            }
        }
        by_id.into_values().filter(|p| p.enabled).collect()
    })
}
pub fn preview(
    file: &FileInfo,
    spec: &Spec,
    config: &Config,
    cancel: &AtomicBool,
) -> Option<(Body, String)> {
    let mut candidates: Vec<_> = manifests().iter().filter(|m| m.score(file) > 0).collect();
    candidates.sort_by(|a, b| {
        b.score(file)
            .cmp(&a.score(file))
            .then_with(|| a.id.cmp(&b.id))
    });
    for manifest in candidates.into_iter().take(3) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        match run(manifest, file, spec, config, cancel) {
            Ok(body) => return Some((body, format!("预览器：{}", manifest.id))),
            Err(error) => eprintln!("预览器 {} 失败，使用下一后端：{error}", manifest.id),
        }
    }
    None
}
fn run(
    manifest: &Manifest,
    file: &FileInfo,
    spec: &Spec,
    config: &Config,
    cancel: &AtomicBool,
) -> Result<Body> {
    manifest.validate()?;
    let temporary = tempfile::Builder::new()
        .prefix("quicklook-plugin-")
        .tempdir()?;
    let output = temporary.path().join(if manifest.output == "pdf" {
        "preview.pdf"
    } else {
        "preview.png"
    });
    let executable = Path::new(&manifest.command[0]);
    let executable = if executable.is_relative() && manifest.command[0].contains('/') {
        manifest.directory.join(executable)
    } else {
        executable.to_path_buf()
    };
    let mut command = Command::new(executable);
    crate::helper::configure(&mut command);
    for arg in manifest.command.iter().skip(1) {
        match arg.as_str() {
            "{path}" => {
                command.arg(&file.path);
            }
            "{uri}" => {
                command.arg(&file.uri);
            }
            "{output}" => {
                command.arg(&output);
            }
            "{page}" => {
                command.arg((spec.page + 1).to_string());
            }
            _ => {
                command.arg(arg);
            }
        }
    }
    let mut child = command
        .current_dir(&manifest.directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法运行插件命令")?;
    let stdout = child.stdout.take().context("无法读取插件输出")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let start = Instant::now();
    let timeout = Duration::from_millis(manifest.timeout_ms.clamp(50, 30_000));
    let mut status = None;
    let mut bytes = None;
    loop {
        if status.is_none() {
            status = child.try_wait()?;
        }
        if bytes.is_none() {
            bytes = receiver.try_recv().ok();
        }
        if status.is_some() && bytes.is_some() {
            break;
        }
        if cancel.load(Ordering::Relaxed) || start.elapsed() > timeout {
            crate::helper::kill(&mut child);
            bail!("插件已取消或超时");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let bytes = bytes.context("插件输出读取失败")??;
    let status = status.context("插件退出状态丢失")?;
    ensure!(status.success(), "插件退出失败");
    ensure!(bytes.len() <= 4 * 1024 * 1024, "插件输出超过 4 MiB");
    match manifest.output.as_str() {
        "text" => {
            let text = String::from_utf8(bytes)?;
            let len = text.len() as u64;
            Ok(Body::Text(TextChunk {
                text,
                encoding: "UTF-8".into(),
                next_offset: len,
                total: len,
                had_errors: false,
            }))
        }
        "html" => Ok(Body::Html(String::from_utf8(bytes)?)),
        "image" => {
            ensure!(
                output.metadata()?.len() <= config.max_extracted_bytes,
                "插件图片超过限额"
            );
            let pixels = crate::native::thumbnail(
                &output,
                spec.edge.unwrap_or(config.image_max_edge),
                cancel,
            )?;
            Ok(Body::Image(ImageData {
                streaming: false,
                source_size: None,
                frames: vec![(pixels, 100)],
                details: "外部预览器".into(),
                exif: Vec::new(),
            }))
        }
        "pdf" => {
            ensure!(
                output.metadata()?.len() <= config.max_extracted_bytes,
                "插件 PDF 超过限额"
            );
            let info = FileInfo::from_path(&output)?;
            Ok(Body::Pdf(crate::backend::pdf(
                &info,
                spec.page,
                spec.edge.unwrap_or(config.image_max_edge),
                cancel,
            )?))
        }
        _ => bail!("无效插件输出格式"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rank_and_literal_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file $(echo injected).custom");
        std::fs::write(&path, "fixture").unwrap();
        let file = FileInfo::from_path(&path).unwrap();
        let exact = Manifest {
            id: "exact".into(),
            extensions: vec!["custom".into()],
            command: vec!["/usr/bin/printf".into(), "%s".into(), "{path}".into()],
            directory: dir.path().to_path_buf(),
            ..Default::default()
        };
        let output = run(
            &exact,
            &file,
            &Spec::default(),
            &Config::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(output,Body::Text(t)if t.text==path.to_string_lossy()));
        let wildcard = Manifest {
            mime_types: vec!["application/*".into()],
            extensions: Vec::new(),
            ..exact.clone()
        };
        assert!(exact.score(&file) > wildcard.score(&file));
    }
    #[test]
    fn timeout_stops_helper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.custom");
        std::fs::write(&path, "fixture").unwrap();
        let file = FileInfo::from_path(&path).unwrap();
        let manifest = Manifest {
            id: "timeout".into(),
            extensions: vec!["custom".into()],
            command: vec!["/usr/bin/sleep".into(), "2".into()],
            directory: dir.path().to_path_buf(),
            timeout_ms: 50,
            ..Default::default()
        };
        let start = Instant::now();
        assert!(
            run(
                &manifest,
                &file,
                &Spec::default(),
                &Config::default(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn timeout_covers_descendants_holding_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.custom");
        std::fs::write(&path, "fixture").unwrap();
        let file = FileInfo::from_path(&path).unwrap();
        let manifest = Manifest {
            id: "forking-helper".into(),
            extensions: vec!["custom".into()],
            command: vec![
                "/usr/bin/python3".into(),
                "-c".into(),
                "import subprocess; subprocess.Popen(['/usr/bin/sleep', '2'])".into(),
            ],
            directory: dir.path().to_path_buf(),
            timeout_ms: 100,
            ..Default::default()
        };
        let start = Instant::now();
        assert!(
            run(
                &manifest,
                &file,
                &Spec::default(),
                &Config::default(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
