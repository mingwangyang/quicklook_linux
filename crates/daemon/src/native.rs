//! Small ownership-checked adapters for the native thumbnail and archive APIs.
use crate::backend::Pixels;
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io::Write,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

unsafe extern "C" {
    fn vips_init(name: *const c_char) -> c_int;
    fn vips_concurrency_set(n: c_int);
    fn vips_cache_set_max(n: c_int);
    fn vips_cache_set_max_mem(n: usize);
    fn vips_thumbnail(path: *const c_char, out: *mut *mut c_void, width: c_int, ...) -> c_int;
    fn vips_image_new_from_file(path: *const c_char, ...) -> *mut c_void;
    fn vips_autorot(input: *mut c_void, out: *mut *mut c_void, ...) -> c_int;
    fn vips_extract_area(
        input: *mut c_void,
        out: *mut *mut c_void,
        left: c_int,
        top: c_int,
        width: c_int,
        height: c_int,
        ...
    ) -> c_int;
    fn vips_resize(input: *mut c_void, out: *mut *mut c_void, scale: f64, ...) -> c_int;
    fn vips_colourspace(input: *mut c_void, out: *mut *mut c_void, space: c_int, ...) -> c_int;
    fn vips_cast(input: *mut c_void, out: *mut *mut c_void, format: c_int, ...) -> c_int;
    fn vips_image_get_width(image: *mut c_void) -> c_int;
    fn vips_image_get_height(image: *mut c_void) -> c_int;
    fn vips_image_get_bands(image: *mut c_void) -> c_int;
    fn vips_image_set_progress(image: *mut c_void, progress: c_int);
    fn vips_image_set_kill(image: *mut c_void, kill: c_int);
    fn vips_image_write_to_memory(image: *mut c_void, size: *mut usize) -> *mut c_void;
    fn vips_error_buffer() -> *const c_char;
    fn vips_error_clear();
    fn archive_read_new() -> *mut c_void;
    fn archive_read_support_filter_all(a: *mut c_void) -> c_int;
    fn archive_read_support_format_all(a: *mut c_void) -> c_int;
    fn archive_read_open_filename(a: *mut c_void, name: *const c_char, block: usize) -> c_int;
    fn archive_read_next_header(a: *mut c_void, e: *mut *mut c_void) -> c_int;
    fn archive_read_data_skip(a: *mut c_void) -> c_int;
    fn archive_read_data(a: *mut c_void, buffer: *mut c_void, size: usize) -> isize;
    fn archive_read_free(a: *mut c_void) -> c_int;
    fn archive_entry_pathname(e: *mut c_void) -> *const c_char;
    fn archive_entry_pathname_utf8(e: *mut c_void) -> *const c_char;
    fn archive_entry_size(e: *mut c_void) -> i64;
    fn archive_entry_mtime(e: *mut c_void) -> i64;
    fn archive_entry_filetype(e: *mut c_void) -> u32;
    fn archive_entry_hardlink(e: *mut c_void) -> *const c_char;
    fn archive_error_string(a: *mut c_void) -> *const c_char;
    fn libraw_init(flags: u32) -> *mut c_void;
    fn libraw_open_file(raw: *mut c_void, file: *const c_char) -> c_int;
    fn libraw_unpack_thumb(raw: *mut c_void) -> c_int;
    fn libraw_unpack(raw: *mut c_void) -> c_int;
    fn libraw_dcraw_process(raw: *mut c_void) -> c_int;
    fn libraw_dcraw_make_mem_image(raw: *mut c_void, error: *mut c_int) -> *mut RawImage;
    fn libraw_get_raw_height(raw: *mut c_void) -> c_int;
    fn libraw_get_raw_width(raw: *mut c_void) -> c_int;
    fn libraw_set_output_bps(raw: *mut c_void, value: c_int);
    fn libraw_set_output_color(raw: *mut c_void, value: c_int);
    fn libraw_set_demosaic(raw: *mut c_void, value: c_int);
    fn libraw_set_progress_handler(
        raw: *mut c_void,
        callback: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int) -> c_int>,
        data: *mut c_void,
    );
    fn libraw_dcraw_make_mem_thumb(raw: *mut c_void, error: *mut c_int) -> *mut RawImage;
    fn libraw_dcraw_clear_mem(image: *mut RawImage);
    fn libraw_close(raw: *mut c_void);
}
#[repr(C)]
struct RawImage {
    kind: c_int,
    height: u16,
    width: u16,
    colors: u16,
    bits: u16,
    size: u32,
    data: [u8; 1],
}
struct GObject(*mut c_void);
impl Drop for GObject {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { glib::gobject_ffi::g_object_unref(self.0.cast()) }
        }
    }
}
static VIPS_LOCK: Mutex<()> = Mutex::new(());
static VIPS_INIT: OnceLock<bool> = OnceLock::new();
fn ensure_vips() -> Result<()> {
    unsafe {
        ensure!(
            *VIPS_INIT.get_or_init(|| {
                let ok = vips_init(c"quicklook".as_ptr()) == 0;
                vips_concurrency_set(1);
                vips_cache_set_max(0);
                vips_cache_set_max_mem(16 * 1024 * 1024);
                ok
            }),
            "无法初始化 libvips"
        );
    }
    Ok(())
}

pub fn thumbnail(path: &Path, edge: u32, cancel: &std::sync::atomic::AtomicBool) -> Result<Pixels> {
    let _guard = VIPS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // All vararg names are NUL terminated; every returned GObject is released.
    unsafe {
        ensure_vips()?;
        ensure!(!cancel.load(std::sync::atomic::Ordering::Relaxed), "已取消");
        let name = CString::new(path.as_os_str().as_bytes())?;
        let mut pointer = std::ptr::null_mut();
        if vips_thumbnail(
            name.as_ptr(),
            &mut pointer,
            edge as c_int,
            c"height".as_ptr(),
            edge as c_int,
            c"size".as_ptr(),
            2i32,
            std::ptr::null::<c_char>(),
        ) != 0
        {
            let message = native_string(vips_error_buffer());
            vips_error_clear();
            bail!("{message}");
        }
        let input = GObject(pointer);
        pixels_from_vips(&input, cancel)
    }
}
// The graph is evaluated only for the requested region. libvips handles JPEG
// shrink-on-load and spills large intermediate images to its temporary cache.
pub fn dimensions(path: &Path) -> Result<[u32; 2]> {
    match vips_dimensions(path) {
        Ok(size) => Ok(size),
        Err(error) => builtin_dimensions(path).with_context(|| format!("{error}")),
    }
}
fn vips_dimensions(path: &Path) -> Result<[u32; 2]> {
    let _guard = VIPS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let input = oriented_image(path)?;
    unsafe {
        Ok([
            vips_image_get_width(input.0) as u32,
            vips_image_get_height(input.0) as u32,
        ])
    }
}
//: Formats the bundled decoder handles on its own. Some libvips builds ship
//: without a BMP or ICO loader, so these stay available either way.
fn builtin_format(path: &Path) -> bool {
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "jpe" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff"
    )
}
fn builtin_dimensions(path: &Path) -> Result<[u32; 2]> {
    ensure!(builtin_format(path), "没有内置解码器");
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let (width, height) = reader.into_dimensions()?;
    Ok([width, height])
}
/// Decodes a still image with the bundled `image` crate. Used when libvips has
/// no loader for the format or fails outright, so a missing vips module cannot
/// take BMP, ICO and friends down with it.
pub fn builtin_thumbnail(
    path: &Path,
    edge: u32,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Pixels> {
    ensure!(!cancel.load(std::sync::atomic::Ordering::Relaxed), "已取消");
    ensure!(builtin_format(path), "没有内置解码器");
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode()?;
    let decoded = if decoded.width() > edge || decoded.height() > edge {
        decoded.thumbnail(edge, edge)
    } else {
        decoded
    };
    let image = decoded.into_rgba8();
    Ok(Pixels::rgba(
        image.width(),
        image.height(),
        image.into_raw(),
    ))
}
fn oriented_image(path: &Path) -> Result<GObject> {
    ensure_vips()?;
    let name = CString::new(path.as_os_str().as_bytes())?;
    unsafe {
        let input = GObject(vips_image_new_from_file(
            name.as_ptr(),
            std::ptr::null::<c_char>(),
        ));
        ensure!(!input.0.is_null(), "无法读取图像头");
        let mut pointer = std::ptr::null_mut();
        ensure!(
            vips_autorot(input.0, &mut pointer, std::ptr::null::<c_char>()) == 0,
            "无法读取图像方向"
        );
        Ok(GObject(pointer))
    }
}
pub fn region(
    path: &Path,
    rect: [u32; 4],
    edge: u32,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Pixels> {
    let _guard = VIPS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let input = oriented_image(path)?;
    unsafe {
        let width = vips_image_get_width(input.0).max(0) as u32;
        let height = vips_image_get_height(input.0).max(0) as u32;
        ensure!(
            rect[2] > 0
                && rect[3] > 0
                && rect[0].saturating_add(rect[2]) <= width
                && rect[1].saturating_add(rect[3]) <= height,
            "图像区域无效"
        );
        let mut pointer = std::ptr::null_mut();
        ensure!(
            vips_extract_area(
                input.0,
                &mut pointer,
                rect[0] as i32,
                rect[1] as i32,
                rect[2] as i32,
                rect[3] as i32,
                std::ptr::null::<c_char>()
            ) == 0,
            "无法读取图像区域"
        );
        let crop = GObject(pointer);
        let scale = (edge as f64 / rect[2].max(rect[3]) as f64).min(1.);
        if scale < 1. {
            let mut pointer = std::ptr::null_mut();
            ensure!(
                vips_resize(crop.0, &mut pointer, scale, std::ptr::null::<c_char>()) == 0,
                "无法缩放图像区域"
            );
            pixels_from_vips(&GObject(pointer), cancel)
        } else {
            pixels_from_vips(&crop, cancel)
        }
    }
}
fn pixels_from_vips(input: &GObject, cancel: &std::sync::atomic::AtomicBool) -> Result<Pixels> {
    unsafe {
        let mut pointer = std::ptr::null_mut();
        if vips_colourspace(input.0, &mut pointer, 22, std::ptr::null::<c_char>()) != 0 {
            vips_error_clear();
            bail!("图像色彩空间不受支持");
        }
        let srgb = GObject(pointer);
        let mut pointer = std::ptr::null_mut();
        ensure!(
            vips_cast(srgb.0, &mut pointer, 0, std::ptr::null::<c_char>()) == 0,
            "无法转换图像像素"
        );
        let output = GObject(pointer);
        let width = vips_image_get_width(output.0);
        let height = vips_image_get_height(output.0);
        let bands = vips_image_get_bands(output.0);
        ensure!(
            width > 0 && height > 0 && width <= 8192 && height <= 8192 && (3..=4).contains(&bands),
            "图像尺寸或通道数无效"
        );
        let mut len = 0;
        unsafe extern "C" fn progress(image: *mut c_void, _: *mut c_void, data: *mut c_void) {
            unsafe {
                if (&*data.cast::<std::sync::atomic::AtomicBool>())
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    vips_image_set_kill(image, 1);
                }
            }
        }
        vips_image_set_progress(output.0, 1);
        let signal = glib::gobject_ffi::g_signal_connect_data(
            output.0.cast(),
            c"eval".as_ptr(),
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void),
                unsafe extern "C" fn(),
            >(progress)),
            (cancel as *const std::sync::atomic::AtomicBool)
                .cast_mut()
                .cast(),
            None,
            0,
        );
        let data = vips_image_write_to_memory(output.0, &mut len);
        glib::gobject_ffi::g_signal_handler_disconnect(output.0.cast(), signal);
        ensure!(!data.is_null(), "图像解码失败");
        let bytes = std::slice::from_raw_parts(data.cast::<u8>(), len);
        let result = if len == width as usize * height as usize * bands as usize {
            let rgba = if bands == 4 {
                bytes.to_vec()
            } else {
                bytes
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect()
            };
            Ok(Pixels::rgba(width as u32, height as u32, rgba))
        } else {
            Err(anyhow::anyhow!("像素缓冲区长度无效"))
        };
        glib::ffi::g_free(data);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_region_preserves_pixels_and_rejects_overflow() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        image::RgbaImage::from_fn(120, 80, |x, y| image::Rgba([x as u8, y as u8, 30, 255]))
            .save(&path)
            .unwrap();
        // Header reading must initialize vips independently of thumbnail().
        assert_eq!(dimensions(&path).unwrap(), [120, 80]);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let tile = region(&path, [20, 30, 40, 20], 2048, &cancel).unwrap();
        assert_eq!((tile.width, tile.height), (40, 20));
        assert_eq!(&tile.bytes[..4], &[20, 30, 30, 255]);
        assert!(region(&path, [u32::MAX, 0, 2, 1], 2048, &cancel).is_err());
    }
}
pub fn raw_thumbnail(
    path: &Path,
    edge: u32,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Pixels> {
    struct Raw(*mut c_void);
    impl Drop for Raw {
        fn drop(&mut self) {
            unsafe { libraw_close(self.0) }
        }
    }
    struct Image(*mut RawImage);
    impl Drop for Image {
        fn drop(&mut self) {
            unsafe { libraw_dcraw_clear_mem(self.0) }
        }
    }
    unsafe {
        let pointer = libraw_init(0);
        ensure!(!pointer.is_null(), "无法初始化 RAW 解码器");
        let raw = Raw(pointer);
        let name = CString::new(path.as_os_str().as_bytes())?;
        ensure!(
            libraw_open_file(raw.0, name.as_ptr()) == 0,
            "无法读取 RAW 文件"
        );
        unsafe extern "C" fn progress(data: *mut c_void, _: c_int, _: c_int, _: c_int) -> c_int {
            unsafe { &*data.cast::<std::sync::atomic::AtomicBool>() }
                .load(std::sync::atomic::Ordering::Relaxed) as c_int
        }
        libraw_set_progress_handler(
            raw.0,
            Some(progress),
            (cancel as *const std::sync::atomic::AtomicBool)
                .cast_mut()
                .cast(),
        );
        let embedded = libraw_unpack_thumb(raw.0) == 0;
        if !embedded {
            let width = libraw_get_raw_width(raw.0);
            let height = libraw_get_raw_height(raw.0);
            ensure!(
                width > 0 && height > 0 && (width as u64) * (height as u64) <= 64_000_000,
                "RAW 无内嵌预览且超过完整解码的 64MP 限额"
            );
            libraw_set_output_bps(raw.0, 8);
            libraw_set_output_color(raw.0, 1);
            libraw_set_demosaic(raw.0, 0);
            ensure!(
                libraw_unpack(raw.0) == 0 && libraw_dcraw_process(raw.0) == 0,
                "RAW 完整解码失败或已取消"
            );
        }
        let mut error = 0;
        let pointer = if embedded {
            libraw_dcraw_make_mem_thumb(raw.0, &mut error)
        } else {
            libraw_dcraw_make_mem_image(raw.0, &mut error)
        };
        ensure!(!pointer.is_null() && error == 0, "RAW 预览图解码失败");
        let image = Image(pointer);
        let data = &*image.0;
        ensure!(data.size <= 128 * 1024 * 1024, "RAW 内嵌预览图过大");
        let bytes = std::slice::from_raw_parts(
            std::ptr::addr_of!(data.data).cast::<u8>(),
            data.size as usize,
        );
        let mut decoded = if data.kind == 1 {
            image::load_from_memory(bytes)?
        } else {
            ensure!(data.bits == 8 && data.colors == 3, "RAW 位图格式暂不支持");
            image::DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(data.width as u32, data.height as u32, bytes.to_vec())
                    .context("RAW 像素长度无效")?,
            )
        };
        if embedded && let Some(exif) = crate::backend::read_exif(path) {
            let value = exif
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
                .unwrap_or(1);
            if let Some(orientation) = image::metadata::Orientation::from_exif(value as u8) {
                decoded.apply_orientation(orientation);
            }
        }
        let decoded = if decoded.width() > edge || decoded.height() > edge {
            decoded.thumbnail(edge, edge)
        } else {
            decoded
        };
        let decoded = decoded.into_rgba8();
        Ok(Pixels::rgba(
            decoded.width(),
            decoded.height(),
            decoded.into_raw(),
        ))
    }
}

#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub modified: i64,
    pub directory: bool,
    pub safe: bool,
}
struct Archive(*mut c_void);
impl Drop for Archive {
    fn drop(&mut self) {
        unsafe {
            archive_read_free(self.0);
        }
    }
}
impl Archive {
    fn open(path: &Path) -> Result<Self> {
        unsafe {
            let a = Self(archive_read_new());
            ensure!(!a.0.is_null(), "无法初始化压缩包读取器");
            archive_read_support_filter_all(a.0);
            archive_read_support_format_all(a.0);
            let name = CString::new(path.as_os_str().as_bytes())?;
            ensure!(
                archive_read_open_filename(a.0, name.as_ptr(), 65536) == 0,
                "{}",
                native_string(archive_error_string(a.0))
            );
            Ok(a)
        }
    }
    fn next(&self) -> Result<Option<*mut c_void>> {
        unsafe {
            let mut entry = std::ptr::null_mut();
            let code = archive_read_next_header(self.0, &mut entry);
            if code == 1 {
                return Ok(None);
            }
            ensure!(
                code >= -20 && !entry.is_null(),
                "{}",
                native_string(archive_error_string(self.0))
            );
            Ok(Some(entry))
        }
    }
}
pub fn archive_list(
    path: &Path,
    max: usize,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(Vec<ArchiveEntry>, bool)> {
    let archive = Archive::open(path)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    while let Some(entry) = archive.next()? {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("已取消");
        }
        if entries.len() >= max {
            truncated = true;
            break;
        }
        unsafe {
            let utf8 = archive_entry_pathname_utf8(entry);
            let name = native_string(if utf8.is_null() {
                archive_entry_pathname(entry)
            } else {
                utf8
            });
            let filetype = archive_entry_filetype(entry);
            entries.push(ArchiveEntry {
                index: entries.len(),
                safe: quicklook_core::files::safe_archive_path(&name)
                    && filetype == 0o100000
                    && archive_entry_hardlink(entry).is_null(),
                name,
                size: archive_entry_size(entry).max(0) as u64,
                modified: archive_entry_mtime(entry),
                directory: filetype == 0o040000,
            });
            archive_read_data_skip(archive.0);
        }
    }
    Ok((entries, truncated))
}
/// 一次性读取压缩包里匹配 `want` 的普通文件，用于 OOXML（docx/xlsx/pptx）这类
/// 「一堆小 XML 打成一个包」的格式。只走一遍 libarchive，总量受 `max` 限制。
pub fn archive_members(
    path: &Path,
    max: usize,
    want: &dyn Fn(&str) -> bool,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<(String, Vec<u8>)>> {
    let archive = Archive::open(path)?;
    let mut members = Vec::new();
    let mut total = 0usize;
    while let Some(entry) = archive.next()? {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("已取消");
        }
        unsafe {
            let utf8 = archive_entry_pathname_utf8(entry);
            let name = native_string(if utf8.is_null() {
                archive_entry_pathname(entry)
            } else {
                utf8
            });
            let file = archive_entry_filetype(entry) == 0o100000
                && archive_entry_hardlink(entry).is_null();
            if !file || !want(&name) {
                archive_read_data_skip(archive.0);
                continue;
            }
            let mut data = Vec::new();
            let mut buffer = [0u8; 65536];
            loop {
                ensure!(!cancel.load(std::sync::atomic::Ordering::Relaxed), "已取消");
                let size = archive_read_data(archive.0, buffer.as_mut_ptr().cast(), buffer.len());
                ensure!(
                    size >= 0,
                    "{}",
                    native_string(archive_error_string(archive.0))
                );
                if size == 0 {
                    break;
                }
                total = total.saturating_add(size as usize);
                ensure!(total <= max, "包内文档结构超过读取上限");
                data.extend_from_slice(&buffer[..size as usize]);
            }
            members.push((name, data));
        }
    }
    Ok(members)
}
pub fn archive_extract(
    path: &Path,
    index: usize,
    max: u64,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(PathBuf, Arc<std::path::PathBuf>)> {
    let archive = Archive::open(path)?;
    let mut current = 0;
    while let Some(entry) = archive.next()? {
        if current != index {
            unsafe {
                archive_read_data_skip(archive.0);
            }
            current += 1;
            continue;
        }
        unsafe {
            let name = native_string(archive_entry_pathname(entry));
            ensure!(
                quicklook_core::files::safe_archive_path(&name),
                "压缩包条目路径不安全"
            );
            ensure!(
                archive_entry_filetype(entry) == 0o100000
                    && archive_entry_hardlink(entry).is_null(),
                "只预览普通文件，不展开链接或设备"
            );
            ensure!(
                archive_entry_size(entry) <= max as i64,
                "包内文件超过解压限额"
            );
            let staging = quicklook_core::config::cache_dir().join("extracted");
            std::fs::create_dir_all(&staging)?;
            let used: u64 = std::fs::read_dir(&staging)?
                .flatten()
                .take(10_000)
                .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .fold(0u64, u64::saturating_add);
            ensure!(
                used.saturating_add(archive_entry_size(entry).max(0) as u64) <= 512 * 1024 * 1024,
                "包内文件暂存区已满，请清理 ~/.cache/quicklook/extracted"
            );
            let directory = tempfile::Builder::new()
                .prefix("quicklook-")
                .tempdir_in(staging)?;
            let output = directory
                .path()
                .join(Path::new(&name).file_name().context("无效文件名")?);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)?;
            let mut buffer = [0u8; 65536];
            let mut total = 0u64;
            loop {
                ensure!(!cancel.load(std::sync::atomic::Ordering::Relaxed), "已取消");
                let size = archive_read_data(archive.0, buffer.as_mut_ptr().cast(), buffer.len());
                ensure!(
                    size >= 0,
                    "{}",
                    native_string(archive_error_string(archive.0))
                );
                if size == 0 {
                    break;
                }
                total += size as u64;
                ensure!(
                    total <= max && used.saturating_add(total) <= 512 * 1024 * 1024,
                    "包内文件超过解压限额"
                );
                file.write_all(&buffer[..size as usize])?;
            }
            return Ok((output, Arc::new(directory.keep())));
        }
    }
    bail!("找不到包内文件")
}
unsafe fn native_string(pointer: *const c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}
