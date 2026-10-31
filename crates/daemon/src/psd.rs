//! Read the flattened PSD image while seeking past resources and layer data.
use crate::backend::Pixels;
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub fn composite(path: &Path, edge: u32, cancel: &AtomicBool) -> Result<Pixels> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut header = [0u8; 26];
    file.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"8BPS" && u16::from_be_bytes(header[4..6].try_into()?) == 1,
        "不是 PSD v1"
    );
    let channels = u16::from_be_bytes(header[12..14].try_into()?) as usize;
    let height = u32::from_be_bytes(header[14..18].try_into()?) as usize;
    let width = u32::from_be_bytes(header[18..22].try_into()?) as usize;
    let bits = u16::from_be_bytes(header[22..24].try_into()?);
    let mode = u16::from_be_bytes(header[24..26].try_into()?);
    ensure!(
        bits == 8 && matches!(mode, 1 | 3),
        "PSD 快路径支持 8-bit RGB/灰度合成图"
    );
    ensure!(
        (1..=16).contains(&channels)
            && width > 0
            && height > 0
            && width <= 300_000
            && height <= 300_000,
        "PSD 尺寸无效"
    );
    for _ in 0..3 {
        let bytes = read_u32(&mut file)? as u64;
        let position = file.stream_position()?;
        ensure!(position + bytes <= length, "PSD 区段越界");
        file.seek(SeekFrom::Current(bytes as i64))?;
    }
    let compression = read_u16(&mut file)?;
    ensure!(compression <= 1, "此 PSD 压缩方式交给原生后端");
    let step = width
        .div_ceil(edge as usize)
        .max(height.div_ceil(edge as usize))
        .max(1);
    let outw = width.div_ceil(step);
    let outh = height.div_ceil(step);
    ensure!(outw * outh <= 64 * 1024 * 1024, "PSD 输出超过限额");
    let mut output = vec![255u8; outw * outh * 4];
    let color_channels = if mode == 1 { 1 } else { 3 };
    ensure!(channels >= color_channels, "PSD 通道数无效");
    let used = channels.min(color_channels + 1);
    let mut row = vec![0u8; width];
    if compression == 0 {
        let base = file.stream_position()?;
        ensure!(
            base + (width as u64) * (height as u64) * (channels as u64) <= length,
            "PSD 合成图不完整"
        );
        for channel in 0..used {
            for y in (0..height).step_by(step) {
                ensure!(!cancel.load(Ordering::Relaxed), "已取消");
                file.seek(SeekFrom::Start(
                    base + ((channel * height + y) * width) as u64,
                ))?;
                file.read_exact(&mut row)?;
                write_row(
                    &mut output,
                    &row,
                    channel,
                    color_channels,
                    y / step,
                    outw,
                    step,
                );
            }
        }
    } else {
        ensure!(channels * height <= 8_000_000, "PSD RLE 索引过大");
        let mut sizes = Vec::with_capacity(channels * height);
        for _ in 0..channels * height {
            sizes.push(read_u16(&mut file)? as usize);
        }
        for (channel, rows) in sizes.chunks(height).enumerate() {
            for (y, &size) in rows.iter().enumerate() {
                ensure!(!cancel.load(Ordering::Relaxed), "已取消");
                if channel < used && y % step == 0 {
                    let mut encoded = vec![0; size];
                    file.read_exact(&mut encoded)?;
                    decode_packbits(&encoded, &mut row)?;
                    write_row(
                        &mut output,
                        &row,
                        channel,
                        color_channels,
                        y / step,
                        outw,
                        step,
                    );
                } else {
                    file.seek(SeekFrom::Current(size as i64))?;
                }
            }
        }
    }
    Ok(Pixels::rgba(outw as u32, outh as u32, output))
}
fn write_row(
    output: &mut [u8],
    row: &[u8],
    channel: usize,
    color_channels: usize,
    y: usize,
    width: usize,
    step: usize,
) {
    for (x, &value) in row.iter().step_by(step).enumerate() {
        let pixel = &mut output[(y * width + x) * 4..][..4];
        if color_channels == 1 && channel == 0 {
            pixel[..3].fill(value);
        } else if channel < color_channels {
            pixel[channel] = value;
        } else {
            pixel[3] = value;
        }
    }
}
fn decode_packbits(source: &[u8], output: &mut [u8]) -> Result<()> {
    let (mut i, mut j) = (0, 0);
    while i < source.len() && j < output.len() {
        let command = source[i] as i8;
        i += 1;
        if command >= 0 {
            let count = command as usize + 1;
            ensure!(
                i + count <= source.len() && j + count <= output.len(),
                "PSD RLE 越界"
            );
            output[j..j + count].copy_from_slice(&source[i..i + count]);
            i += count;
            j += count;
        } else if command != -128 {
            let count = (1 - command as i16) as usize;
            ensure!(
                i < source.len() && j + count <= output.len(),
                "PSD RLE 越界"
            );
            output[j..j + count].fill(source[i]);
            i += 1;
            j += count;
        }
    }
    ensure!(j == output.len(), "PSD RLE 行不完整");
    Ok(())
}
fn read_u16(file: &mut File) -> Result<u16> {
    let mut b = [0; 2];
    file.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}
fn read_u32(file: &mut File) -> Result<u32> {
    let mut b = [0; 4];
    file.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packbits_limits() {
        let mut output = [0; 6];
        decode_packbits(&[1, 10, 20, 253, 30], &mut output).unwrap();
        assert_eq!(output, [10, 20, 30, 30, 30, 30]);
        assert!(decode_packbits(&[127, 1], &mut output).is_err());
    }
}
