use anyhow::Result;
use encoding_rs::Encoding;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct TextChunk {
    pub text: String,
    pub encoding: String,
    pub next_offset: u64,
    pub total: u64,
    pub had_errors: bool,
}
pub fn is_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return true;
    }
    let controls = bytes
        .iter()
        .filter(|&&b| b == 0 || (b < 32 && !matches!(b, 9 | 10 | 12 | 13 | 27)))
        .count();
    controls * 100 <= bytes.len().max(1)
}
pub fn read_chunk(
    path: &Path,
    offset: u64,
    limit: usize,
    label: Option<&str>,
) -> Result<TextChunk> {
    let mut file = File::open(path)?;
    let total = file.metadata()?.len();
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take((limit + 4) as u64).read_to_end(&mut bytes)?;
    let (encoding, bom) = if offset == 0 {
        Encoding::for_bom(&bytes).unwrap_or_else(|| (detect(&bytes, label), 0))
    } else {
        (detect(&bytes, label), 0)
    };
    let available = bytes.len().saturating_sub(bom);
    let mut count = available.min(limit);
    let mut decoded = None;
    if offset + (bytes.len() as u64) < total || count < available {
        for trim in 0..=4.min(count) {
            let end = bom + count - trim;
            if let Some(text) =
                encoding.decode_without_bom_handling_and_without_replacement(&bytes[bom..end])
            {
                decoded = Some(text.into_owned());
                count -= trim;
                break;
            }
        }
    }
    let (text, had_errors) = match decoded {
        Some(s) => (s, false),
        None => {
            let (s, e) = encoding.decode_without_bom_handling(&bytes[bom..bom + count]);
            (s.into_owned(), e)
        }
    };
    Ok(TextChunk {
        text,
        encoding: encoding.name().into(),
        next_offset: offset + (bom + count) as u64,
        total,
        had_errors,
    })
}
fn detect(bytes: &[u8], label: Option<&str>) -> &'static Encoding {
    if let Some(e) = label.and_then(|s| Encoding::for_label(s.as_bytes())) {
        return e;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return encoding_rs::UTF_8;
    }
    // Truncated UTF-8 at the end of the sample is not evidence for a legacy encoding.
    if let Err(e) = std::str::from_utf8(bytes)
        && e.error_len().is_none()
    {
        return encoding_rs::UTF_8;
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    detector.feed(bytes, false);
    detector.guess(Some(b"cn"), chardetng::Utf8Detection::Allow)
}
pub fn hex_summary(bytes: &[u8]) -> String {
    let mut s = String::new();
    for (offset, row) in bytes.chunks(16).take(128).enumerate() {
        s.push_str(&format!("{:08x}  ", offset * 16));
        for i in 0..16 {
            if let Some(b) = row.get(i) {
                s.push_str(&format!("{b:02x} "));
            } else {
                s.push_str("   ");
            }
        }
        s.push(' ');
        for &b in row {
            s.push(if (32..=126).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
        s.push('\n');
    }
    s.push_str("\n可打印字符串（文件头，最多 32 项）：\n");
    for text in bytes
        .split(|b| !(32..=126).contains(b))
        .filter(|s| s.len() >= 4)
        .take(32)
    {
        s.push_str(&String::from_utf8_lossy(&text[..text.len().min(160)]));
        s.push('\n');
    }
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multibyte_page_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.txt");
        for encoding in [encoding_rs::UTF_8, encoding_rs::GB18030] {
            let value = "你好𠀀世界\n".repeat(30);
            let (bytes, _, _) = encoding.encode(&value);
            std::fs::write(&path, &bytes).unwrap();
            let mut offset = 0;
            let mut result = String::new();
            while offset < bytes.len() as u64 {
                let chunk = read_chunk(&path, offset, 17, Some(encoding.name())).unwrap();
                assert!(!chunk.had_errors);
                assert!(chunk.next_offset > offset);
                offset = chunk.next_offset;
                result.push_str(&chunk.text);
            }
            assert_eq!(result, value);
        }
    }
    #[test]
    fn binary_not_text() {
        assert!(!is_text(&[0, 0, 0, 0, 1, 2, 3, 4]));
        assert!(is_text("你好\n".as_bytes()));
    }
}
