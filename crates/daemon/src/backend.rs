use anyhow::{Context, Result, bail, ensure};
use glib::translate::ToGlibPtr;
use gtk4::cairo;
use quicklook_core::{
    config::Config,
    files::{FileInfo, Kind},
    text::{TextChunk, hex_summary, is_text, read_chunk},
};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub bytes: Arc<[u8]>,
    pub bgra: bool,
}
impl Pixels {
    pub fn rgba(width: u32, height: u32, bytes: Vec<u8>) -> Self {
        Self {
            width,
            height,
            bytes: bytes.into(),
            bgra: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ImageData {
    pub streaming: bool,
    pub frames: Vec<(Pixels, u32)>,
    pub details: String,
    pub exif: Vec<(String, String)>,
    pub source_size: Option<[u32; 2]>,
}
#[derive(Clone, Debug)]
pub struct PdfData {
    pub pixels: Pixels,
    pub page: i32,
    pub total: i32,
    pub text: String,
    pub boxes: Vec<[f64; 4]>,
}
#[derive(Clone, Debug)]
pub struct DirectoryData {
    pub entries: Vec<FileInfo>,
    pub total_bytes: u64,
    pub truncated: bool,
}
#[derive(Clone, Debug)]
pub struct FontData {
    pub family: String,
    pub glyphs: u16,
    pub samples: String,
}
#[derive(Clone, Debug)]
pub enum Body {
    Tile {
        pixels: Pixels,
        rect: [u32; 4],
    },
    Image(ImageData),
    Pdf(PdfData),
    Text(TextChunk),
    /// CSV / TSV：解析成表格再渲染，避免纯文本视图列不对齐。
    Csv(Box<crate::csv::CsvDoc>),
    Markdown {
        source: TextChunk,
        html: String,
        /// Markdown 里是否有 mermaid 图；只有这种情况才给 WebView 打开 JavaScript。
        diagrams: bool,
    },
    Rtf {
        source: TextChunk,
        html: String,
    },
    Html(String),
    Structured {
        source: TextChunk,
        value: serde_json::Value,
    },
    Directory(DirectoryData),
    Archive {
        entries: Vec<crate::native::ArchiveEntry>,
        truncated: bool,
    },
    Binary(String),
    Summary(String),
    Media,
    Font(FontData),
    Office(Box<crate::office::OfficeDoc>),
}
#[derive(Clone, Debug)]
pub struct Prepared {
    pub info: FileInfo,
    pub body: Body,
    pub note: String,
    pub temporary: Option<Arc<std::path::PathBuf>>,
}
impl Prepared {
    pub fn weight(&self) -> usize {
        match &self.body {
            Body::Image(i) => i.frames.iter().map(|f| f.0.bytes.len()).sum(),
            Body::Tile { pixels, .. } => pixels.bytes.len(),
            Body::Pdf(p) => p.pixels.bytes.len() + p.text.len() + p.boxes.len() * 32,
            Body::Text(t) => t.text.len(),
            Body::Markdown { source, html, .. } | Body::Rtf { source, html } => {
                source.text.len() + html.len()
            }
            Body::Html(h) | Body::Binary(h) | Body::Summary(h) => h.len(),
            Body::Structured { source, .. } => source.text.len() * 4,
            Body::Directory(d) => d.entries.len() * 512,
            Body::Archive { entries, .. } => entries.len() * 256,
            Body::Office(doc) => doc.weight(),
            Body::Csv(doc) => doc.weight(),
            _ => 1024,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Spec {
    pub page: i32,
    pub edge: Option<u32>,
    pub encoding: Option<String>,
    pub offset: u64,
}
impl Spec {
    pub fn key(&self, info: &FileInfo) -> String {
        format!(
            "{}:{:?}:{}:{}:{}",
            info.key(),
            self.edge,
            self.page,
            self.offset,
            self.encoding.as_deref().unwrap_or("auto")
        )
    }
}

pub fn prepare(
    info: FileInfo,
    spec: &Spec,
    config: &Config,
    cancel: &AtomicBool,
) -> Result<Prepared> {
    ensure!(!cancel.load(Ordering::Relaxed), "已取消");
    if let Some((body, note)) = crate::plugins::preview(&info, spec, config, cancel) {
        return Ok(Prepared {
            info,
            body,
            note,
            temporary: None,
        });
    }
    let mut temporary: Option<Arc<std::path::PathBuf>> = None;
    let mut note = String::new();
    let edge = spec.edge.unwrap_or(config.image_max_edge);
    let body = match info.kind {
        Kind::Image | Kind::Psd => {
            let ext = info
                .path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let webp_animated = if ext == "webp" {
                let mut header = [0u8; 21];
                File::open(&info.path)
                    .and_then(|mut f| f.read_exact(&mut header))
                    .is_ok()
                    && &header[12..16] == b"VP8X"
                    && header[20] & 2 != 0
            } else {
                false
            };
            let animated = (ext == "gif" || webp_animated)
                && info.size <= 32 * 1024 * 1024
                && spec.edge.is_none();
            let frames = if animated {
                match animation(&info.path, edge.min(1600), cancel) {
                    Ok(frames) if !frames.is_empty() => Some(frames),
                    Err(error) => {
                        ensure!(!cancel.load(Ordering::Relaxed), "已取消");
                        note = format!("动画预解码降级：{error}");
                        None
                    }
                    _ => None,
                }
            } else {
                None
            };
            if ext == "gif" && spec.edge.is_none() && frames.is_none() {
                return Ok(Prepared {
                    info,
                    body: Body::Media,
                    note: "长动画 · libmpv 流式播放".into(),
                    temporary: None,
                });
            }
            let streaming = webp_animated
                && spec.edge.is_none()
                && frames.is_none()
                && info.size <= 128 * 1024 * 1024;
            if streaming {
                note = "WebP 动画 · 按帧解码，双帧缓冲".into();
            }
            let frames = if let Some(frames) = frames {
                frames
            } else {
                let pixels = if info.kind == Kind::Psd {
                    crate::psd::composite(&info.path, edge, cancel)
                        .or_else(|_| crate::native::thumbnail(&info.path, edge, cancel))?
                } else {
                    match crate::native::thumbnail(&info.path, edge, cancel) {
                        Ok(pixels) => pixels,
                        Err(error) => {
                            ensure!(!cancel.load(Ordering::Relaxed), "已取消");
                            let pixels = crate::native::builtin_thumbnail(&info.path, edge, cancel)
                                .with_context(|| format!("libvips：{error}"))?;
                            note = format!("libvips 缺少该格式的解码器，已用内置解码器：{error}");
                            pixels
                        }
                    }
                };
                vec![(pixels, 100)]
            };
            let source_size = if frames.len() == 1 && !streaming && info.kind == Kind::Image {
                crate::native::dimensions(&info.path).ok()
            } else {
                None
            };
            let dimensions = source_size.map(|[w, h]| (w, h));
            let details = dimensions
                .map(|(w, h)| format!("{w} × {h} 像素"))
                .unwrap_or_else(|| {
                    format!("{} × {} 预览像素", frames[0].0.width, frames[0].0.height)
                });
            Body::Image(ImageData {
                streaming,
                source_size,
                frames,
                details,
                exif: exif_fields(&info.path),
            })
        }
        Kind::Raw => {
            let pixels = crate::native::raw_thumbnail(&info.path, edge, cancel)
                .or_else(|_| crate::native::thumbnail(&info.path, edge, cancel))?;
            Body::Image(ImageData {
                streaming: false,
                source_size: None,
                details: "相机 RAW · 内嵌预览图优先".into(),
                frames: vec![(pixels, 100)],
                exif: exif_fields(&info.path),
            })
        }
        Kind::Pdf => Body::Pdf(pdf(&info, spec.page, edge, cancel)?),
        Kind::Eps => {
            let path = convert_eps(&info, cancel)?;
            let mut converted = FileInfo::from_path(&path)?;
            converted.kind = Kind::Pdf;
            note = "EPS → 缓存 PDF".into();
            Body::Pdf(pdf(&converted, spec.page, edge, cancel)?)
        }
        Kind::Media => Body::Media,
        Kind::Directory => {
            let mut entries = Vec::new();
            let mut total_bytes = 0u64;
            let mut truncated = false;
            for entry in std::fs::read_dir(&info.path)? {
                if cancel.load(Ordering::Relaxed) {
                    bail!("已取消");
                }
                if entries.len() >= 20_000 {
                    truncated = true;
                    break;
                }
                if let Ok(file) = entry.map(|e| e.path())
                    && let Ok(file) = FileInfo::from_path(&file)
                {
                    if file.kind != Kind::Directory {
                        total_bytes = total_bytes.saturating_add(file.size);
                    }
                    entries.push(file);
                }
            }
            entries.sort_by(|a, b| {
                (a.kind != Kind::Directory)
                    .cmp(&(b.kind != Kind::Directory))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            Body::Directory(DirectoryData {
                entries,
                total_bytes,
                truncated,
            })
        }
        Kind::Archive => {
            let (entries, truncated) =
                crate::native::archive_list(&info.path, config.max_archive_entries, cancel)?;
            Body::Archive { entries, truncated }
        }
        Kind::Office => {
            let doc = crate::office::OfficeDoc::read(&info.path, cancel)?;
            note = doc.note.clone();
            Body::Office(Box::new(doc))
        }
        Kind::Text | Kind::Markdown | Kind::Html | Kind::Structured | Kind::Rtf => {
            // CSV / TSV 要看整张表才谈得上「优化」，给到 4 MiB；普通文本仍按配置分块。
            let separated = if info.kind == Kind::Text {
                crate::csv::delimiter_for(&info.name, &info.mime)
            } else {
                None
            };
            let limit = if separated.is_some() {
                config.text_chunk_bytes.max(4 * 1024 * 1024)
            } else if matches!(info.kind, Kind::Html | Kind::Structured | Kind::Rtf) {
                config.text_chunk_bytes.max(2 * 1024 * 1024)
            } else {
                config.text_chunk_bytes
            };
            let chunk = read_chunk(&info.path, spec.offset, limit, spec.encoding.as_deref())?;
            if let Some(preferred) = separated {
                let partial = chunk.next_offset < chunk.total;
                let read = quicklook_core::files::format_size(chunk.text.len() as u64);
                let doc = crate::csv::read(chunk, preferred);
                let mut notes: Vec<String> = Vec::new();
                if partial {
                    notes.push(format!("仅读取前 {read}"));
                }
                let warnings = doc.warnings();
                if !warnings.is_empty() {
                    notes.push(warnings);
                }
                note = notes.join(" · ");
                return Ok(Prepared {
                    info,
                    body: Body::Csv(Box::new(doc)),
                    note,
                    temporary: None,
                });
            }
            match info.kind {
                Kind::Markdown => {
                    let diagrams = crate::mermaid::has_diagram(&chunk.text);
                    let mut html = String::new();
                    pulldown_cmark::html::push_html(
                        &mut html,
                        pulldown_cmark::Parser::new_ext(
                            &chunk.text,
                            pulldown_cmark::Options::ENABLE_TABLES
                                | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
                                | pulldown_cmark::Options::ENABLE_TASKLISTS,
                        ),
                    );
                    Body::Markdown {
                        source: chunk,
                        html,
                        diagrams,
                    }
                }
                Kind::Html => {
                    if chunk.next_offset < chunk.total {
                        note = "仅显示 HTML 前 2 MiB".into();
                    }
                    Body::Html(chunk.text)
                }
                Kind::Structured => {
                    let ext = info
                        .path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    let value = match ext.as_str() {
                        "yaml" | "yml" => {
                            serde_yaml::from_str::<serde_json::Value>(&chunk.text).ok()
                        }
                        "xml" => xml_value(&chunk.text).ok(),
                        _ => serde_json::from_str(&chunk.text).ok(),
                    };
                    if let Some(value) = value {
                        Body::Structured {
                            source: chunk,
                            value,
                        }
                    } else {
                        note = "结构解析未完成或失败，显示源码".into();
                        Body::Text(chunk)
                    }
                }
                Kind::Rtf => {
                    let html = crate::rtf::html(&chunk.text);
                    Body::Rtf {
                        source: chunk,
                        html,
                    }
                }
                _ => Body::Text(chunk),
            }
        }
        Kind::Font => {
            ensure!(info.size <= 64 * 1024 * 1024, "字体文件超过 64 MiB 限额");
            let bytes = std::fs::read(&info.path)?;
            let face = ttf_parser::Face::parse(&bytes, 0)
                .map_err(|_| anyhow::anyhow!("字体格式暂不支持"))?;
            let family = face
                .names()
                .into_iter()
                .find(|n| n.name_id == ttf_parser::name_id::FAMILY)
                .and_then(|n| n.to_string())
                .unwrap_or_else(|| info.name.clone());
            let samples = face
                .tables()
                .cmap
                .into_iter()
                .flat_map(|t| t.subtables)
                .filter(|s| s.is_unicode())
                .fold(String::new(), |mut all, s| {
                    s.codepoints(|cp| {
                        if all.chars().count() < 512
                            && let Some(c) = char::from_u32(cp)
                            && !c.is_control()
                            && face.glyph_index(c).is_some()
                            && !all.contains(c)
                        {
                            all.push(c);
                            all.push(' ');
                        }
                    });
                    all
                });
            Body::Font(FontData {
                family,
                glyphs: face.number_of_glyphs(),
                samples,
            })
        }
        Kind::Binary => {
            let mut bytes = Vec::new();
            File::open(&info.path)?.take(8192).read_to_end(&mut bytes)?;
            if is_text(&bytes) {
                Body::Text(read_chunk(
                    &info.path,
                    0,
                    config.text_chunk_bytes,
                    spec.encoding.as_deref(),
                )?)
            } else {
                Body::Binary(hex_summary(&bytes))
            }
        }
    };
    // Temporary extraction ownership is attached by the worker before delivery.
    Ok(Prepared {
        info,
        body,
        note,
        temporary: temporary.take(),
    })
}
fn animation(path: &Path, edge: u32, cancel: &AtomicBool) -> Result<Vec<(Pixels, u32)>> {
    use image::{AnimationDecoder, ImageDecoder};
    let reader = BufReader::new(File::open(path)?);
    let ext = path.extension().unwrap_or_default().to_string_lossy();
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(96 * 1024 * 1024);
    let frames = if ext.eq_ignore_ascii_case("gif") {
        let mut decoder = image::codecs::gif::GifDecoder::new(reader)?;
        decoder.set_limits(limits)?;
        decoder.into_frames()
    } else {
        let mut decoder = image::codecs::webp::WebPDecoder::new(reader)?;
        decoder.set_limits(limits)?;
        decoder.into_frames()
    };
    let mut out = Vec::new();
    let mut used = 0;
    for frame in frames.take(241) {
        ensure!(out.len() < 240, "动画超过 240 帧，使用流式播放或静态预览");
        ensure!(!cancel.load(Ordering::Relaxed), "已取消");
        let frame = frame?;
        let (a, b) = frame.delay().numer_denom_ms();
        let decoded = image::DynamicImage::ImageRgba8(frame.into_buffer());
        let decoded = if decoded.width() > edge || decoded.height() > edge {
            decoded.thumbnail(edge, edge)
        } else {
            decoded
        };
        let image = decoded.into_rgba8();
        used += image.as_raw().len();
        ensure!(used <= 96 * 1024 * 1024, "动画超出内存预算");
        out.push((
            Pixels::rgba(image.width(), image.height(), image.into_raw()),
            (a / b.max(1)).max(16),
        ));
    }
    Ok(out)
}
/// The bounded channel keeps long animated WebP files out of the frame cache.
pub fn stream_webp(
    path: std::path::PathBuf,
    edge: u32,
    cancel: Arc<AtomicBool>,
) -> std::sync::mpsc::Receiver<Result<(Pixels, u32), String>> {
    use image::{AnimationDecoder, ImageDecoder};
    let (sender, receiver) = std::sync::mpsc::sync_channel(2);
    std::thread::Builder::new()
        .name("quicklook-webp".into())
        .spawn(move || {
            let result = (|| -> Result<()> {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    let mut decoder =
                        image::codecs::webp::WebPDecoder::new(BufReader::new(File::open(&path)?))?;
                    let animated = decoder.has_animation();
                    let mut limits = image::Limits::default();
                    limits.max_alloc = Some(96 * 1024 * 1024);
                    decoder.set_limits(limits)?;
                    let mut count = 0;
                    for frame in decoder.into_frames() {
                        if cancel.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        let frame = frame?;
                        let (n, d) = frame.delay().numer_denom_ms();
                        let decoded = image::DynamicImage::ImageRgba8(frame.into_buffer());
                        let decoded = if decoded.width() > edge || decoded.height() > edge {
                            decoded.thumbnail(edge, edge)
                        } else {
                            decoded
                        };
                        let pixels = decoded.into_rgba8();
                        if sender
                            .send(Ok((
                                Pixels::rgba(pixels.width(), pixels.height(), pixels.into_raw()),
                                (n / d.max(1)).max(16),
                            )))
                            .is_err()
                        {
                            return Ok(());
                        }
                        count += 1;
                    }
                    if !animated || count == 0 {
                        return Ok(());
                    }
                }
            })();
            if let Err(error) = result {
                let _ = sender.send(Err(format!("{error:#}")));
            }
        })
        .expect("无法创建动画解码线程");
    receiver
}
pub fn pdf(info: &FileInfo, index: i32, edge: u32, cancel: &AtomicBool) -> Result<PdfData> {
    let document = poppler::Document::from_file(&info.uri, None)
        .context("PDF 无法读取（可能已加密或 AI 未包含 PDF 兼容内容）")?;
    let total = document.n_pages();
    ensure!(total > 0, "PDF 无页面");
    let index = index.clamp(0, total - 1);
    let page = document.page(index).context("无法读取 PDF 页面")?;
    let (w, h) = page.size();
    ensure!(
        w > 0. && h > 0. && w.is_finite() && h.is_finite(),
        "PDF 页面大小无效"
    );
    let scale = (edge as f64 / w.max(h)).min(2.5);
    let width = (w * scale).ceil().max(1.) as i32;
    let height = (h * scale).ceil().max(1.) as i32;
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    {
        let context = cairo::Context::new(&surface)?;
        context.set_source_rgb(1., 1., 1.);
        context.paint()?;
        context.scale(scale, scale);
        ensure!(!cancel.load(Ordering::Relaxed), "已取消");
        page.render(&context);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    {
        let bytes = surface.data().map_err(|e| anyhow::anyhow!(e.to_string()))?;
        for row in bytes.chunks(stride) {
            pixels.extend_from_slice(&row[..width as usize * 4]);
        }
    }
    let text = page.text().map(|s| s.to_string()).unwrap_or_default();
    let mut rectangles = std::ptr::null_mut();
    let mut count = 0u32;
    let mut boxes = Vec::new();
    unsafe {
        if poppler::ffi::poppler_page_get_text_layout(
            page.to_glib_none().0,
            &mut rectangles,
            &mut count,
        ) != 0
            && !rectangles.is_null()
        {
            for r in std::slice::from_raw_parts(rectangles, count.min(1_000_000) as usize) {
                boxes.push([r.x1 * scale, r.y1 * scale, r.x2 * scale, r.y2 * scale]);
            }
            glib::ffi::g_free(rectangles.cast());
        }
    }
    Ok(PdfData {
        pixels: Pixels {
            width: width as u32,
            height: height as u32,
            bytes: pixels.into(),
            bgra: cfg!(target_endian = "little"),
        },
        page: index,
        total,
        text,
        boxes,
    })
}
pub fn extract(
    info: &FileInfo,
    index: usize,
    config: &Config,
    cancel: &AtomicBool,
) -> Result<Prepared> {
    let (path, directory) =
        crate::native::archive_extract(&info.path, index, config.max_extracted_bytes, cancel)?;
    let file = FileInfo::from_path(&path)?;
    let mut content = prepare(file, &Spec::default(), config, cancel)?;
    content.note = format!("{} › {}", info.name, content.info.name);
    content.temporary = Some(directory);
    Ok(content)
}
fn convert_eps(info: &FileInfo, cancel: &AtomicBool) -> Result<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    info.key().hash(&mut hash);
    let directory = quicklook_core::config::cache_dir().join("eps");
    std::fs::create_dir_all(&directory)?;
    let output = directory.join(format!("{:016x}.pdf", hash.finish()));
    if output.is_file() {
        return Ok(output);
    }
    let temp = tempfile::NamedTempFile::new_in(&directory)?;
    let mut command = Command::new("gs");
    crate::helper::configure(&mut command);
    let mut child = command
        .args([
            "-dSAFER",
            "-dBATCH",
            "-dNOPAUSE",
            "-dEPSCrop",
            "-sDEVICE=pdfwrite",
        ])
        .arg(format!("-sOutputFile={}", temp.path().display()))
        .arg("-f")
        .arg(&info.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Ghostscript 转换失败");
            break;
        }
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(20) {
            crate::helper::kill(&mut child);
            bail!("EPS 转换超时或取消");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    temp.persist(&output)?;
    Ok(output)
}
pub fn read_exif(path: &Path) -> Option<exif::Exif> {
    // TIFF metadata readers may read the entire container. Keep metadata work
    // bounded even when the image itself is many gigabytes.
    struct Budget<R> {
        inner: R,
        left: usize,
    }
    impl<R: Read> Read for Budget<R> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let limit = bytes.len().min(self.left);
            let n = self.inner.read(&mut bytes[..limit])?;
            self.left -= n;
            Ok(n)
        }
    }
    impl<R: std::io::Seek> std::io::Seek for Budget<R> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }
    let reader = Budget {
        inner: File::open(path).ok()?,
        left: 8 * 1024 * 1024,
    };
    exif::Reader::new()
        .read_from_container(&mut BufReader::new(reader))
        .ok()
}
fn exif_fields(path: &Path) -> Vec<(String, String)> {
    let Some(exif) = read_exif(path) else {
        return Vec::new();
    };
    exif.fields()
        .take(80)
        .map(|field| {
            (
                field.tag.to_string(),
                field
                    .display_value()
                    .with_unit(&exif)
                    .to_string()
                    .chars()
                    .take(512)
                    .collect(),
            )
        })
        .collect()
}
pub fn directory_stats(info: FileInfo, cancel: &AtomicBool) -> Result<Prepared> {
    let start = Instant::now();
    let mut paths = vec![(info.path.clone(), 0)];
    let (mut files, mut folders, mut size, mut newest) = (0u64, 0u64, 0u64, 0u64);
    let mut complete = true;
    while let Some((path, depth)) = paths.pop() {
        if cancel.load(Ordering::Relaxed) {
            bail!("已取消");
        }
        if files + folders >= 100_000 || start.elapsed() > Duration::from_secs(3) {
            complete = false;
            break;
        }
        let entries = match std::fs::read_dir(path) {
            Ok(e) => e,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                bail!("已取消");
            }
            if files + folders >= 100_000 || start.elapsed() > Duration::from_secs(3) {
                complete = false;
                break;
            }
            let Ok(kind) = entry.file_type() else {
                complete = false;
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                newest = newest.max(
                    metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|t| t.as_secs())
                        .unwrap_or(0),
                );
                if kind.is_dir() {
                    folders += 1;
                    if depth < 64 && paths.len() < 10_000 {
                        paths.push((entry.path(), depth + 1));
                    } else {
                        complete = false;
                    }
                } else if kind.is_file() {
                    files += 1;
                    size = size.saturating_add(metadata.len());
                }
            }
        }
    }
    let modified = glib::DateTime::from_unix_local(newest as i64)
        .ok()
        .and_then(|d| d.format("%Y-%m-%d %H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let summary = format!(
        "{files} 个文件 · {folders} 个子目录 · 总大小 {}{}\n最近修改：{modified} · 不跟随符号链接",
        quicklook_core::files::format_size(size),
        if complete { "" } else { "（部分统计）" }
    );
    Ok(Prepared {
        info,
        body: Body::Summary(summary),
        note: String::new(),
        temporary: None,
    })
}
fn xml_value(text: &str) -> Result<serde_json::Value> {
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            nodes_limit: 50_000,
            ..Default::default()
        },
    )?;
    fn node(n: roxmltree::Node<'_, '_>, depth: usize) -> serde_json::Value {
        if depth > 64 {
            return serde_json::json!("…深度限制");
        }
        let mut out = serde_json::Map::new();
        for a in n.attributes() {
            out.insert(
                format!("@{}", a.name()),
                serde_json::Value::String(a.value().into()),
            );
        }
        let children: Vec<_> = n
            .children()
            .filter(|c| c.is_element())
            .map(|c| serde_json::json!({c.tag_name().name():node(c,depth+1)}))
            .collect();
        if !children.is_empty() {
            out.insert("children".into(), children.into());
        }
        if let Some(t) = n.text().map(str::trim).filter(|s| !s.is_empty()) {
            out.insert("text".into(), t.into());
        }
        out.into()
    }
    Ok(
        serde_json::json!({document.root_element().tag_name().name():node(document.root_element(),0)}),
    )
}
