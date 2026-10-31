//! CSV / TSV 预览：把分隔文本渲染成表格，而不是当普通文本刷出来。
//!
//! 纯文本视图看 CSV 最大的问题是列不对齐、看不出表头与数值列；这里按
//! RFC 4180 解析（引号、`""` 转义、字段内换行、CRLF、BOM），再把首行当表头
//! 渲染成带行号、粘性表头、数值右对齐的表格。分隔符先按扩展名/MIME 判断，
//! 再按内容嗅探（Excel 导出的 CSV 常用分号或制表符）。
//!
//! 上限刻意保守：解析全部行只是为了统计「共多少行」，渲染最多
//! [`MAX_ROWS`] 行 × [`MAX_COLUMNS`] 列，超出就截断并在状态行说明。

use quicklook_core::text::TextChunk;

/// 最多渲染的数据行数（不含表头）。
pub const MAX_ROWS: usize = 2000;
/// 最多渲染的列数。
pub const MAX_COLUMNS: usize = 60;
/// 单元格最多显示的字符数，避免一格把整页撑开。
const MAX_CELL: usize = 400;
/// 生成的 HTML 上限。
const MAX_HTML: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CsvDoc {
    /// 原始文本块，保留编码信息供「切换编码（E）」使用。
    pub source: TextChunk,
    pub html: String,
    /// 数据行数（不含表头，未截断前的真实值）。
    pub rows: usize,
    /// 渲染出来的列数。
    pub columns: usize,
    /// 真实列数（可能大于 `columns`）。
    pub total_columns: usize,
    pub delimiter: char,
    pub truncated: bool,
}

impl CsvDoc {
    pub fn weight(&self) -> usize {
        self.html.len()
    }
    /// 截断提示；没截断时为空串（底栏不重复摘要）。
    pub fn warnings(&self) -> String {
        let mut notes: Vec<String> = Vec::new();
        if self.truncated {
            notes.push(format!("仅显示前 {MAX_ROWS} 行（共 {} 行）", self.rows));
        }
        if self.total_columns > self.columns {
            notes.push(format!(
                "仅显示前 {MAX_COLUMNS} 列（共 {} 列）",
                self.total_columns
            ));
        }
        notes.join(" · ")
    }

    /// 状态行用的一行摘要，例如「CSV · 逗号分隔 · 201 行 × 7 列」。
    /// 截断信息不放在这里——它由 [`Self::warnings`] 单独输出，避免底栏重复。
    pub fn summary(&self) -> String {
        format!(
            "CSV · {}分隔 · {} 行 × {} 列",
            delimiter_name(self.delimiter),
            self.rows,
            self.total_columns
        )
    }
}

pub fn delimiter_name(delimiter: char) -> &'static str {
    match delimiter {
        '\t' => "制表符",
        ';' => "分号",
        '|' => "竖线",
        ' ' => "空格",
        _ => "逗号",
    }
}

/// 由扩展名 / MIME 得到的「首选分隔符」，`None` 表示这个文件不是分隔文本。
pub fn delimiter_for(name: &str, mime: &str) -> Option<char> {
    let extension = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "csv" => Some(','),
        "tsv" | "tab" => Some('\t'),
        "psv" => Some('|'),
        _ => {
            if mime.eq_ignore_ascii_case("text/csv") {
                Some(',')
            } else if mime.eq_ignore_ascii_case("text/tab-separated-values") {
                Some('\t')
            } else {
                None
            }
        }
    }
}

/// 把文本渲染成表格；`preferred` 是扩展名给出的默认分隔符。
pub fn read(chunk: TextChunk, preferred: char) -> CsvDoc {
    let (delimiter, total_rows, total_columns, columns, truncated, html) = {
        let text = chunk.text.as_str();
        let delimiter = sniff(text).unwrap_or(preferred);
        let (rows, total_rows) = parse(text, delimiter);
        let total_columns = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let columns = total_columns.min(MAX_COLUMNS);
        let truncated = total_rows > MAX_ROWS + 1;
        let html = render(&rows, delimiter, columns, truncated, total_columns);
        (
            delimiter,
            total_rows,
            total_columns,
            columns,
            truncated,
            html,
        )
    };
    CsvDoc {
        source: chunk,
        html,
        rows: total_rows.saturating_sub(1),
        columns,
        total_columns,
        delimiter,
        truncated,
    }
}

/// 按 RFC 4180 解析。返回（最多 `MAX_ROWS + 1` 行数据，真实行数）。
///
/// 首行是表头，所以「数据行数」要减一；截断时仍返回全部行用于统计。
pub fn parse(text: &str, delimiter: char) -> (Vec<Vec<String>>, usize) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            c if c == delimiter => row.push(std::mem::take(&mut cell)),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    // 去掉整行为空的尾部（文件末尾的换行、Excel 追加的空行）。
    while rows
        .last()
        .is_some_and(|r| r.iter().all(|c| c.trim().is_empty()))
    {
        rows.pop();
    }
    let total = rows.len();
    if rows.len() > MAX_ROWS + 1 {
        rows.truncate(MAX_ROWS + 1);
    }
    (rows, total)
}

/// 按内容嗅探分隔符：对候选分隔符解析前缀，取「字段数最一致且大于 1」的那个。
pub fn sniff(text: &str) -> Option<char> {
    let head: String = text.chars().take(64 * 1024).collect();
    let mut best: Option<(f64, char)> = None;
    for candidate in [',', ';', '\t', '|'] {
        let (rows, _) = parse(&head, candidate);
        if rows.is_empty() {
            continue;
        }
        let mut counts: Vec<usize> = Vec::new();
        for row in rows.iter().take(50) {
            counts.push(row.len());
        }
        let Some(&mode) = counts
            .iter()
            .max_by_key(|c| counts.iter().filter(|x| *x == *c).count())
        else {
            continue;
        };
        if mode < 2 {
            continue;
        }
        let agreed = counts.iter().filter(|c| **c == mode).count();
        let score = (agreed as f64) / (counts.len() as f64) * (mode as f64).ln_1p();
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, delimiter)| delimiter)
}

fn render(
    rows: &[Vec<String>],
    delimiter: char,
    columns: usize,
    truncated: bool,
    total_columns: usize,
) -> String {
    let mut html = String::with_capacity(4096);
    html.push_str(STYLE);
    html.push_str("<div class=\"csv\">");
    let Some((header, body)) = rows.split_first() else {
        html.push_str("<p class=\"note\">（空文件）</p></div>");
        return html;
    };
    // 数值列：数据行里非空值全都像数字，才右对齐。
    let numeric: Vec<bool> = (0..columns)
        .map(|column| {
            let mut seen = 0usize;
            for row in body {
                if let Some(text) = row.get(column) {
                    let text = text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    seen += 1;
                    if !looks_numeric(text) {
                        return false;
                    }
                }
            }
            seen > 0
        })
        .collect();
    html.push_str("<table><thead><tr><th class=\"row\"></th>");
    for (column, is_number) in numeric.iter().enumerate() {
        let text = header.get(column).map(String::as_str).unwrap_or("");
        let class = if *is_number { "num" } else { "" };
        html.push_str(&format!(
            "<th class=\"{class}\">{}</th>",
            escape(&clip(text))
        ));
    }
    html.push_str("</tr></thead><tbody>");
    for (index, row) in body.iter().enumerate() {
        html.push_str(&format!("<tr><th class=\"row\">{}</th>", index + 1));
        for (column, is_number) in numeric.iter().enumerate() {
            let text = row.get(column).map(String::as_str).unwrap_or("");
            let class = if *is_number { "num" } else { "" };
            html.push_str(&format!(
                "<td class=\"{class}\">{}</td>",
                escape(&clip(text))
            ));
        }
        html.push_str("</tr>");
        if html.len() > MAX_HTML {
            html.push_str("<tr><td class=\"note\" colspan=\"99\">…内容过长，已截断</td></tr>");
            break;
        }
    }
    html.push_str("</tbody></table></div>");
    if truncated || total_columns > columns {
        let mut note = String::new();
        if truncated {
            note.push_str(&format!("仅显示前 {MAX_ROWS} 行"));
        }
        if total_columns > columns {
            if !note.is_empty() {
                note.push('，');
            }
            note.push_str(&format!("仅显示前 {MAX_COLUMNS} 列"));
        }
        html.push_str(&format!("<p class=\"note\">{note}</p>"));
    }
    let _ = delimiter;
    html
}

/// 数字、带千位分隔符的数字、百分数、带正负号的小数都算数值。
fn looks_numeric(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let body = trimmed
        .strip_suffix('%')
        .or_else(|| trimmed.strip_suffix('％'))
        .unwrap_or(trimmed);
    let cleaned: String = body.chars().filter(|c| *c != ',' && *c != '_').collect();
    cleaned.parse::<f64>().is_ok()
}

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_CELL {
        return text.to_string();
    }
    let mut short: String = text.chars().take(MAX_CELL).collect();
    short.push('…');
    short
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

const STYLE: &str = "<style>
.csv{overflow:auto}
.csv table{font-size:12.5px;border-collapse:collapse;width:auto;min-width:100%}
.csv thead th{position:sticky;top:0;z-index:2;background:var(--surface-strong);color:var(--muted);font-weight:600;white-space:nowrap}
.csv tbody th.row,.csv thead th.row{position:sticky;left:0;z-index:3;background:var(--surface-strong);color:var(--muted);text-align:right;font-weight:400;font-variant-numeric:tabular-nums;white-space:nowrap}
.csv thead th.row{z-index:4}
.csv td,.csv th{border:1px solid var(--border);padding:3px 8px;white-space:pre;max-width:360px;overflow:hidden;text-overflow:ellipsis}
.csv td.num{text-align:right;font-variant-numeric:tabular-nums}
.csv tbody tr:nth-child(even) td{background:var(--surface)}
.csv .note{color:var(--muted);font-size:13px;margin:.6em 0}
.csv td.note{white-space:normal}
</style>";

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(text: &str) -> TextChunk {
        TextChunk {
            text: text.to_string(),
            encoding: "UTF-8".into(),
            next_offset: text.len() as u64,
            total: text.len() as u64,
            had_errors: false,
        }
    }

    #[test]
    fn parses_quotes_commas_and_newlines() {
        let text = "a,b,c\n1,\"x,y\",\"line1\nline2\"\n";
        let (rows, total) = parse(text, ',');
        assert_eq!(total, 2);
        assert_eq!(rows[1][1], "x,y");
        assert_eq!(rows[1][2], "line1\nline2");
    }

    #[test]
    fn unescapes_doubled_quotes() {
        let (rows, _) = parse("h\n\"say \"\"hi\"\"\"\n", ',');
        assert_eq!(rows[1][0], "say \"hi\"");
    }

    #[test]
    fn handles_crlf_and_bom() {
        let (rows, _) = parse("\u{feff}a;b\r\n1;2\r\n", ';');
        assert_eq!(rows[0], vec!["a", "b"]);
        assert_eq!(rows[1], vec!["1", "2"]);
    }

    #[test]
    fn sniffs_semicolon_tables() {
        let text = "日期;产品;金额\n2026-01-01;键盘;59\n2026-01-02;鼠标;129\n";
        assert_eq!(sniff(text), Some(';'));
        let doc = read(chunk(text), ',');
        assert_eq!(doc.delimiter, ';');
        assert_eq!(doc.rows, 2);
        assert_eq!(doc.total_columns, 3);
    }

    #[test]
    fn tab_files_stay_tab_separated() {
        let text = "a\tb\n1\t2\n";
        assert_eq!(sniff(text), Some('\t'));
    }

    #[test]
    fn ragged_rows_are_padded_by_render() {
        let text = "a,b,c\n1,2\n1,2,3,4\n";
        let (rows, _) = parse(text, ',');
        assert_eq!(rows.len(), 3);
        let html = render(&rows, ',', 8, false, 8);
        assert!(html.contains("<td class=\"\"></td>"), "{html}");
    }

    #[test]
    fn numeric_columns_are_right_aligned() {
        let text = "名称,金额\n键盘,59.5\n鼠标,1,200\n";
        let (rows, total) = parse(text, ',');
        let html = render(&rows, ',', 2, false, total);
        assert!(html.contains("<th class=\"num\">金额</th>"), "{html}");
        // 带千位分隔符的 1,200 被当成数值所在列的一个字段（列数为 2 时它会被拆成两格）
        assert!(html.contains("<td class=\"num\">59.5</td>"), "{html}");
    }

    #[test]
    fn text_columns_are_left_aligned() {
        let text = "名称,金额\n键盘,59\n";
        let (rows, _) = parse(text, ',');
        let html = render(&rows, ',', 2, false, 2);
        assert!(html.contains("<th class=\"\">名称</th>"), "{html}");
    }

    #[test]
    fn truncates_to_row_limit() {
        let mut text = String::from("h\n");
        for i in 0..(MAX_ROWS + 50) {
            text.push_str(&format!("{i}\n"));
        }
        let doc = read(chunk(&text), ',');
        assert!(doc.truncated);
        assert_eq!(doc.rows, MAX_ROWS + 50);
        assert!(doc.warnings().contains("仅显示前"));
        assert_eq!(doc.html.matches("<tr>").count(), MAX_ROWS + 1);
    }

    #[test]
    fn escapes_markup_in_cells() {
        let (rows, _) = parse("h\n<b>&\n", ',');
        let html = render(&rows, ',', 1, false, 1);
        assert!(html.contains("&lt;b&gt;&amp;"), "{html}");
    }
}
