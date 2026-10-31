//! Bounded, read-only RTF text/style extraction. Embedded objects stay inert.
#[derive(Clone, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    size: i32,
    skip: bool,
    uc: usize,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            size: 12,
            skip: false,
            uc: 1,
        }
    }
}
pub fn html(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::from("<div style='white-space:pre-wrap'>");
    let mut style = Style::default();
    let mut stack = Vec::new();
    let mut run = String::new();
    let mut current = style.clone();
    let mut index = 0;
    let mut encoded = Vec::new();
    let mut encoding = encoding_rs::WINDOWS_1252;
    let mut high = None;
    fn flush(out: &mut String, run: &mut String, style: &Style) {
        if run.is_empty() {
            return;
        }
        out.push_str(&format!(
            "<span style='font-size:{}pt;font-weight:{};font-style:{};text-decoration:{}'>",
            style.size,
            if style.bold { "700" } else { "400" },
            if style.italic { "italic" } else { "normal" },
            if style.underline { "underline" } else { "none" }
        ));
        for c in run.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                _ => out.push(c),
            }
        }
        out.push_str("</span>");
        run.clear();
    }
    while index < chars.len() {
        if style != current {
            if !encoded.is_empty() {
                run.push_str(&encoding.decode_without_bom_handling(&encoded).0);
                encoded.clear();
            }
            flush(&mut out, &mut run, &current);
            current = style.clone();
        }
        let c = chars[index];
        index += 1;
        if c != '\\' && !encoded.is_empty() {
            run.push_str(&encoding.decode_without_bom_handling(&encoded).0);
            encoded.clear();
        }
        match c {
            '{' => {
                if stack.len() < 256 {
                    stack.push(style.clone());
                } else {
                    style.skip = true;
                }
            }
            '}' => style = stack.pop().unwrap_or_default(),
            '\\' => {
                if index >= chars.len() {
                    break;
                }
                let next = chars[index];
                if next == '\'' && index + 2 < chars.len() {
                    let digits: String = chars[index + 1..index + 3].iter().collect();
                    if !style.skip
                        && let Ok(byte) = u8::from_str_radix(&digits, 16)
                    {
                        encoded.push(byte);
                    }
                    index += 3;
                    continue;
                }
                if !encoded.is_empty() {
                    run.push_str(&encoding.decode_without_bom_handling(&encoded).0);
                    encoded.clear();
                }
                if matches!(next, '\\' | '{' | '}') {
                    if !style.skip {
                        run.push(next);
                    }
                    index += 1;
                    continue;
                }
                if next == '*' {
                    style.skip = true;
                    index += 1;
                    continue;
                }
                let start = index;
                while index < chars.len() && chars[index].is_ascii_alphabetic() {
                    index += 1;
                }
                let word: String = chars[start..index].iter().collect();
                let start = index;
                if index < chars.len() && chars[index] == '-' {
                    index += 1;
                }
                while index < chars.len() && chars[index].is_ascii_digit() {
                    index += 1;
                }
                let number: Option<i32> =
                    chars[start..index].iter().collect::<String>().parse().ok();
                if index < chars.len() && chars[index] == ' ' {
                    index += 1;
                }
                match word.as_str() {
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "object"
                    | "generator" | "fldinst" => style.skip = true,
                    "ansicpg" => {
                        let n = number.unwrap_or(1252);
                        encoding = match n {
                            936 => encoding_rs::GBK,
                            54936 => encoding_rs::GB18030,
                            65001 => encoding_rs::UTF_8,
                            _ => {
                                encoding_rs::Encoding::for_label(format!("windows-{n}").as_bytes())
                                    .unwrap_or(encoding_rs::WINDOWS_1252)
                            }
                        };
                    }
                    "uc" => style.uc = number.unwrap_or(1).clamp(0, 16) as usize,
                    "b" => style.bold = number != Some(0),
                    "i" => style.italic = number != Some(0),
                    "ul" => style.underline = number != Some(0),
                    "ulnone" => style.underline = false,
                    "fs" => style.size = (number.unwrap_or(24) / 2).clamp(6, 96),
                    "plain" => {
                        let skip = style.skip;
                        style = Style {
                            skip,
                            ..Default::default()
                        };
                    }
                    "par" | "line" if !style.skip => run.push('\n'),
                    "tab" if !style.skip => run.push('\t'),
                    "u" if !style.skip => {
                        let unit = number.unwrap_or(0) as i16 as u16;
                        if (0xd800..=0xdbff).contains(&unit) {
                            high = Some(unit);
                        } else {
                            let point = if let Some(previous) = high.take() {
                                if (0xdc00..=0xdfff).contains(&unit) {
                                    0x10000
                                        + ((previous as u32 - 0xd800) << 10)
                                        + (unit as u32 - 0xdc00)
                                } else {
                                    unit as u32
                                }
                            } else {
                                unit as u32
                            };
                            if let Some(c) = char::from_u32(point) {
                                run.push(c);
                            }
                        }
                        for _ in 0..style.uc {
                            if index < chars.len() && !matches!(chars[index], '{' | '}' | '\\') {
                                index += 1;
                            }
                        }
                    }
                    "emdash" if !style.skip => run.push('—'),
                    "endash" if !style.skip => run.push('–'),
                    "bullet" if !style.skip => run.push('•'),
                    _ => {
                        if index == start && index < chars.len() {
                            index += 1;
                        }
                    }
                }
            }
            '\r' | '\n' => {}
            _ if !style.skip => run.push(c),
            _ => {}
        }
    }
    if !encoded.is_empty() {
        run.push_str(&encoding.decode_without_bom_handling(&encoded).0);
    }
    flush(&mut out, &mut run, &current);
    out.push_str("</div>");
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_styles() {
        let output = html(r"{\rtf1\ansi Hello \b bold\b0\par\u20320?\u22909?\u-10176?\u-9216?}");
        assert!(output.contains("font-weight:700"));
        assert!(output.contains("你好𠀀"));
    }
    #[test]
    fn gbk_hex_and_inert_objects() {
        let output = html(r"{\rtf1\ansicpg936 \'c4\'e3\'ba\'c3 {\object malicious} <script>}");
        assert!(output.contains("你好"));
        assert!(!output.contains("malicious"));
        assert!(output.contains("&lt;script&gt;"));
    }
}
