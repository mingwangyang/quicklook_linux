//! Office 预览：docx / xlsx / pptx。
//!
//! OOXML 本质是「ZIP + 若干 XML」，这里只做只读解析，不引入 LibreOffice：
//! - docx：`word/document.xml` 的段落、标题、列表、表格
//! - xlsx：`xl/workbook.xml` + `xl/sharedStrings.xml` + 各 `xl/worksheets/*.xml`
//! - pptx：`ppt/presentation.xml` 的放映顺序 + 各 `ppt/slides/slideN.xml` 的文本
//!
//! 解析结果直接生成语义化 HTML，交给 WebKit 渲染（禁用 JavaScript、CSP 禁止外部资源），
//! 这样表格对齐、长表格滚动都由排版引擎负责，不需要手写 GTK 网格。

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// 文档里允许渲染的最大段落数，避免超长文档把 HTML 撑爆。
const MAX_BLOCKS: usize = 6000;
/// 单个表格最多渲染的行数。
const MAX_ROWS: usize = 400;
/// 表格最多渲染的列数。
const MAX_COLUMNS: usize = 32;
/// 最多渲染的工作表 / 幻灯片数量。
const MAX_PANES: usize = 60;
/// 生成 HTML 的字符上限。
const MAX_HTML: usize = 8 * 1024 * 1024;
/// 从压缩包里读取的 XML 总量上限。
const MAX_XML: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct OfficeDoc {
    pub body: OfficeBody,
    /// 解析过程中被截断的部分，显示在状态行。
    pub note: String,
}
#[derive(Clone, Debug)]
pub enum OfficeBody {
    /// Word：顺序排列的块。
    Document(Vec<Block>),
    /// Excel：多个工作表。
    Sheets(Vec<Sheet>),
    /// PowerPoint：按放映顺序排列的幻灯片。
    Slides(Vec<Slide>),
}
#[derive(Clone, Debug)]
pub enum Block {
    Heading(usize, String),
    Paragraph(String),
    Bullet(String),
    Table(Vec<Vec<String>>),
}
#[derive(Clone, Debug)]
pub struct Sheet {
    pub name: String,
    /// 每行是「列索引 + 文本」，列索引用来还原空单元格的位置。
    pub rows: Vec<Vec<(usize, String)>>,
    pub truncated: bool,
}
#[derive(Clone, Debug)]
pub struct Slide {
    pub title: String,
    pub lines: Vec<String>,
}

impl OfficeDoc {
    /// 解析一个 OOXML 文件。返回的错误直接面向用户。
    pub fn read(path: &Path, cancel: &AtomicBool) -> Result<Self> {
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let members = crate::native::archive_members(
            path,
            MAX_XML,
            &|name| keep_member(&extension, name),
            cancel,
        )
        .with_context(|| {
            format!(
                ".{extension} 应当是一个 ZIP 容器；读取包内 XML 失败，文件可能已损坏，或者只是改了扩展名"
            )
        })?;
        let files: HashMap<String, Vec<u8>> = members
            .into_iter()
            .map(|(name, data)| (name.trim_start_matches("./").to_string(), data))
            .collect();
        let text = |name: &str| -> Option<String> {
            files
                .get(name)
                .map(|data| String::from_utf8_lossy(data).into_owned())
        };
        match extension.as_str() {
            "docx" | "docm" | "dotx" | "dotm" => {
                let document =
                    text("word/document.xml").context("不是有效的 docx：缺少 word/document.xml")?;
                Ok(Self::document(&document))
            }
            "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" => {
                let workbook =
                    text("xl/workbook.xml").context("不是有效的 xlsx：缺少 xl/workbook.xml")?;
                let rels = text("xl/_rels/workbook.xml.rels").unwrap_or_default();
                let shared = text("xl/sharedStrings.xml")
                    .map(|xml| shared_strings(&xml))
                    .unwrap_or_default();
                Self::sheets(&workbook, &rels, &shared, &files, cancel)
            }
            "pptx" | "pptm" | "potx" | "ppsx" | "ppsm" => {
                let presentation = text("ppt/presentation.xml")
                    .context("不是有效的 pptx：缺少 ppt/presentation.xml")?;
                let rels = text("ppt/_rels/presentation.xml.rels").unwrap_or_default();
                Self::slides(&presentation, &rels, &files, cancel)
            }
            _ => bail!("暂不支持的 Office 格式：{extension}"),
        }
    }
    fn document(xml: &str) -> Self {
        let mut blocks = Vec::new();
        let mut note = String::new();
        match parse_document(xml) {
            Ok(mut parsed) => {
                if parsed.len() > MAX_BLOCKS {
                    parsed.truncate(MAX_BLOCKS);
                    note = format!("仅显示前 {MAX_BLOCKS} 段");
                }
                blocks = parsed;
            }
            Err(error) => note = format!("解析失败：{error}"),
        }
        if blocks.is_empty() && note.is_empty() {
            note = "文档没有任何可见文本".into();
        }
        Self {
            body: OfficeBody::Document(blocks),
            note,
        }
    }
    fn sheets(
        workbook: &str,
        rels: &str,
        shared: &[String],
        files: &HashMap<String, Vec<u8>>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let targets = relationships(rels);
        let names = sheet_names(workbook);
        let mut sheets = Vec::new();
        let mut note = String::new();
        for (name, rid) in names.iter() {
            if cancel.load(Ordering::Relaxed) {
                bail!("已取消");
            }
            if sheets.len() >= MAX_PANES {
                note = format!("仅显示前 {MAX_PANES} 个工作表");
                break;
            }
            let Some(target) = targets.get(rid) else {
                continue;
            };
            let path = normalize_part("xl", target);
            let Some(data) = files.get(&path) else {
                continue;
            };
            let xml = String::from_utf8_lossy(data);
            let (rows, truncated) = parse_sheet(&xml, shared);
            sheets.push(Sheet {
                name: name.clone(),
                rows,
                truncated,
            });
        }
        if sheets.is_empty() {
            bail!("工作簿里没有可显示的数据");
        }
        Ok(Self {
            body: OfficeBody::Sheets(sheets),
            note,
        })
    }
    fn slides(
        presentation: &str,
        rels: &str,
        files: &HashMap<String, Vec<u8>>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let targets = relationships(rels);
        let mut parts = slide_order(presentation, &targets);
        if parts.is_empty() {
            // 关系表缺失时退回文件名排序，避免整个文档打不开。
            let mut names: Vec<String> = files
                .keys()
                .filter(|k| k.starts_with("ppt/slides/slide") && k.ends_with(".xml"))
                .cloned()
                .collect();
            names.sort_by(|a, b| crate::office::natural(a, b));
            parts = names;
        }
        let mut slides = Vec::new();
        let mut note = String::new();
        for part in parts {
            if cancel.load(Ordering::Relaxed) {
                bail!("已取消");
            }
            if slides.len() >= MAX_PANES {
                note = format!("仅显示前 {MAX_PANES} 张幻灯片");
                break;
            }
            let Some(data) = files.get(&part) else {
                continue;
            };
            let xml = String::from_utf8_lossy(data);
            slides.push(parse_slide(&xml));
        }
        if slides.is_empty() {
            bail!("演示文稿里没有可显示的文本");
        }
        Ok(Self {
            body: OfficeBody::Slides(slides),
            note,
        })
    }
    /// 粗略估算这份预览在缓存里占多少字节。
    pub fn weight(&self) -> usize {
        match &self.body {
            OfficeBody::Document(blocks) => blocks.iter().map(block_weight).sum(),
            OfficeBody::Sheets(sheets) => sheets
                .iter()
                .map(|s| {
                    s.name.len()
                        + s.rows
                            .iter()
                            .flatten()
                            .map(|(_, t)| t.len() + 8)
                            .sum::<usize>()
                })
                .sum(),
            OfficeBody::Slides(slides) => slides
                .iter()
                .map(|s| s.title.len() + s.lines.iter().map(|l| l.len() + 8).sum::<usize>())
                .sum(),
        }
    }
    /// 生成交给 WebKit 的 HTML 片段（只含 body 内容，样式沿用 web.css）。
    pub fn to_html(&self) -> String {
        let mut html = String::with_capacity(4096);
        html.push_str(OFFICE_STYLE);
        match &self.body {
            OfficeBody::Document(blocks) => {
                html.push_str("<div class=\"doc\">");
                for block in blocks {
                    match block {
                        Block::Heading(level, text) => {
                            let level = (*level).clamp(1, 6);
                            html.push_str(&format!("<h{level}>{}</h{level}>", escape(text)));
                        }
                        Block::Paragraph(text) => {
                            html.push_str(&format!("<p>{}</p>", escape(text)));
                        }
                        Block::Bullet(text) => {
                            html.push_str(&format!("<li>{}</li>", escape(text)));
                        }
                        Block::Table(rows) => {
                            html.push_str("<table>");
                            for (index, row) in rows.iter().enumerate() {
                                html.push_str("<tr>");
                                for cell in row {
                                    let tag = if index == 0 { "th" } else { "td" };
                                    html.push_str(&format!("<{tag}>{}</{tag}>", escape(cell)));
                                }
                                html.push_str("</tr>");
                            }
                            html.push_str("</table>");
                        }
                    }
                    if html.len() > MAX_HTML {
                        html.push_str("<p class=\"note\">…内容过长，已截断</p>");
                        break;
                    }
                }
                html.push_str("</div>");
            }
            OfficeBody::Sheets(sheets) => {
                for sheet in sheets {
                    html.push_str(&format!("<h2>{}</h2>", escape(&sheet.name)));
                    html.push_str("<div class=\"grid\"><table><tr><th class=\"corner\"></th>");
                    let columns = sheet
                        .rows
                        .iter()
                        .flat_map(|row| row.iter().map(|(c, _)| *c))
                        .max()
                        .map(|m| (m + 1).min(MAX_COLUMNS))
                        .unwrap_or(0);
                    for column in 0..columns {
                        html.push_str(&format!("<th>{}</th>", column_name(column)));
                    }
                    html.push_str("</tr>");
                    for (index, row) in sheet.rows.iter().enumerate() {
                        html.push_str(&format!("<tr><th class=\"row\">{}</th>", index + 1));
                        let mut cursor = 0usize;
                        for (column, text) in row {
                            if *column >= columns {
                                continue;
                            }
                            while cursor < *column {
                                html.push_str("<td></td>");
                                cursor += 1;
                            }
                            html.push_str(&format!("<td>{}</td>", escape(text)));
                            cursor += 1;
                        }
                        for _ in cursor..columns {
                            html.push_str("<td></td>");
                        }
                        html.push_str("</tr>");
                    }
                    html.push_str("</table></div>");
                    if sheet.truncated {
                        html.push_str("<p class=\"note\">表格已截断</p>");
                    }
                    if html.len() > MAX_HTML {
                        html.push_str("<p class=\"note\">…内容过长，已截断</p>");
                        break;
                    }
                }
            }
            OfficeBody::Slides(slides) => {
                for (index, slide) in slides.iter().enumerate() {
                    html.push_str(&format!(
                        "<section class=\"slide\"><div class=\"num\">第 {} 张</div>",
                        index + 1
                    ));
                    if !slide.title.is_empty() {
                        html.push_str(&format!("<h3>{}</h3>", escape(&slide.title)));
                    }
                    if !slide.lines.is_empty() {
                        html.push_str("<ul class=\"bullets\">");
                        for line in &slide.lines {
                            html.push_str(&format!("<li>{}</li>", escape(line)));
                        }
                        html.push_str("</ul>");
                    } else if slide.title.is_empty() {
                        html.push_str("<p class=\"note\">（此页没有文本，可能是纯图片）</p>");
                    }
                    html.push_str("</section>");
                    if html.len() > MAX_HTML {
                        html.push_str("<p class=\"note\">…内容过长，已截断</p>");
                        break;
                    }
                }
            }
        }
        html
    }
}
const OFFICE_STYLE: &str = "<style>
.doc p{margin:.5em 0}.doc li{margin:.2em 0 .2em 1.2em}
.grid{overflow:auto;max-height:none}
.grid table{font-size:12px;width:auto;min-width:100%}
.grid td,.grid th{padding:3px 8px;white-space:pre;max-width:360px;overflow:hidden;text-overflow:ellipsis}
.grid th.corner,.grid th.row{background:var(--surface-strong);color:var(--muted);position:sticky;left:0}
.grid th{position:sticky;top:0;background:var(--surface-strong)}
.slide{border:1px solid var(--border);border-radius:10px;padding:14px 18px;margin:14px 0;background:var(--surface)}
.slide h3{margin:.2em 0 .5em}
.slide .num{color:var(--muted);font-size:12px;letter-spacing:.06em}
ul.bullets{margin:.2em 0;padding-left:1.2em}
p.note,.note{color:var(--muted);font-size:13px}
</style>";
fn block_weight(block: &Block) -> usize {
    match block {
        Block::Heading(_, text) | Block::Paragraph(text) | Block::Bullet(text) => text.len() + 16,
        Block::Table(rows) => rows.iter().flatten().map(|c| c.len() + 8).sum(),
    }
}
/// 判断某个包内文件是否需要读进内存。
fn keep_member(extension: &str, name: &str) -> bool {
    let name = name.trim_start_matches("./");
    match extension {
        "docx" | "docm" | "dotx" | "dotm" => {
            name == "word/document.xml" || name == "word/styles.xml"
        }
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" => {
            name == "xl/workbook.xml"
                || name == "xl/sharedStrings.xml"
                || name == "xl/_rels/workbook.xml.rels"
                || name.starts_with("xl/worksheets/")
        }
        "pptx" | "pptm" | "potx" | "ppsx" | "ppsm" => {
            name == "ppt/presentation.xml"
                || name == "ppt/_rels/presentation.xml.rels"
                || name.starts_with("ppt/slides/")
        }
        _ => false,
    }
}
/// 解析 `_rels/*.rels`，得到 rId → 目标部件。
fn relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return map;
    };
    for node in doc.descendants().filter(|n| local(n) == "Relationship") {
        let (Some(id), Some(target)) = (attribute(node, "Id"), attribute(node, "Target")) else {
            continue;
        };
        map.insert(id.to_string(), target.to_string());
    }
    map
}
/// 把关系里的相对路径补全成包内绝对路径。
fn normalize_part(base: &str, target: &str) -> String {
    if let Some(stripped) = target.strip_prefix('/') {
        return stripped.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
/// `xl/workbook.xml` 里的工作表名与关系 id，保持原始顺序。
fn sheet_names(xml: &str) -> Vec<(String, String)> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| local(n) == "sheet")
        .filter_map(|node| {
            let name = attribute(node, "name")?.to_string();
            let rid = attribute(node, "id")?.to_string();
            Some((name, rid))
        })
        .collect()
}
/// `xl/sharedStrings.xml`：每个 `<si>` 里的所有 `<t>` 拼接。
pub fn shared_strings(xml: &str) -> Vec<String> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| local(n) == "si")
        .map(|node| {
            node.descendants()
                .filter(|n| local(n) == "t")
                .filter_map(|t| t.text())
                .collect::<String>()
        })
        .collect()
}
/// 解析一个工作表：返回 (行, 是否截断)。每行是 (列号, 文本)。
pub fn parse_sheet(xml: &str, shared: &[String]) -> (Vec<Vec<(usize, String)>>, bool) {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return (Vec::new(), false);
    };
    let mut rows = Vec::new();
    let mut truncated = false;
    let mut next_row = 0usize;
    for row in doc.descendants().filter(|n| local(n) == "row") {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        let index = attribute(row, "r")
            .and_then(|r| r.parse::<usize>().ok())
            .map(|r| r.saturating_sub(1))
            .unwrap_or(next_row);
        // 空行也保留，否则表格会挤在一起。
        while rows.len() < index.min(MAX_ROWS - 1) {
            rows.push(Vec::new());
        }
        next_row = index + 1;
        let mut cells = Vec::new();
        let mut next_column = 0usize;
        for cell in row.children().filter(|n| local(n) == "c") {
            if cells.len() >= MAX_COLUMNS {
                truncated = true;
                break;
            }
            let column = attribute(cell, "r")
                .map(column_index)
                .filter(|c| *c < MAX_COLUMNS)
                .unwrap_or(next_column);
            next_column = column + 1;
            let kind = attribute(cell, "t").unwrap_or_default();
            let value = match kind {
                "s" => child_text(cell, "v")
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default(),
                "inlineStr" => cell
                    .descendants()
                    .filter(|n| local(n) == "t")
                    .filter_map(|t| t.text())
                    .collect::<String>(),
                "b" => match child_text(cell, "v").as_deref() {
                    Some("1") => "TRUE".to_string(),
                    Some("0") => "FALSE".to_string(),
                    _ => String::new(),
                },
                _ => child_text(cell, "v").unwrap_or_default(),
            };
            if value.is_empty() {
                continue;
            }
            cells.push((column, value));
        }
        rows.push(cells);
    }
    (rows, truncated)
}
/// `ppt/presentation.xml` 的放映顺序 → 幻灯片部件路径。
fn slide_order(xml: &str, targets: &HashMap<String, String>) -> Vec<String> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| local(n) == "sldId")
        .filter_map(|node| attribute(node, "id"))
        .filter_map(|rid| targets.get(rid))
        .map(|target| normalize_part("ppt", target))
        .collect()
}
/// 解析一张幻灯片：第一段当标题，其余段落当要点。
pub fn parse_slide(xml: &str) -> Slide {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Slide {
            title: String::new(),
            lines: Vec::new(),
        };
    };
    let mut paragraphs = Vec::new();
    for paragraph in doc.descendants().filter(|n| local(n) == "p") {
        let text = paragraph
            .descendants()
            .filter(|n| local(n) == "t")
            .filter_map(|t| t.text())
            .collect::<String>();
        let text = text.trim().to_string();
        if !text.is_empty() {
            paragraphs.push(text);
        }
        if paragraphs.len() >= 60 {
            break;
        }
    }
    let title = paragraphs.first().cloned().unwrap_or_default();
    let lines = paragraphs.into_iter().skip(1).collect();
    Slide { title, lines }
}
/// 解析 `word/document.xml`：段落、标题、列表、表格。
pub fn parse_document(xml: &str) -> Result<Vec<Block>> {
    let doc = roxmltree::Document::parse(xml).context("document.xml 不是合法 XML")?;
    let body = doc
        .descendants()
        .find(|n| local(n) == "body")
        .context("document.xml 缺少 body")?;
    let mut blocks = Vec::new();
    for node in body.children().filter(|n| n.is_element()) {
        match local(&node).as_str() {
            "p" => {
                let text = paragraph_text(node);
                if text.trim().is_empty() {
                    continue;
                }
                let level = heading_level(node);
                let bullet = node
                    .children()
                    .filter(|n| local(n) == "pPr")
                    .any(|p| p.descendants().any(|d| local(&d) == "numPr"));
                blocks.push(match (level, bullet) {
                    (Some(level), _) => Block::Heading(level, text),
                    (None, true) => Block::Bullet(text),
                    _ => Block::Paragraph(text),
                });
            }
            "tbl" => {
                let mut rows = Vec::new();
                for row in node.children().filter(|n| local(n) == "tr") {
                    if rows.len() >= MAX_ROWS {
                        break;
                    }
                    let cells: Vec<String> = row
                        .children()
                        .filter(|n| local(n) == "tc")
                        .take(MAX_COLUMNS)
                        .map(|cell| {
                            cell.children()
                                .filter(|n| local(n) == "p")
                                .map(paragraph_text)
                                .filter(|t| !t.trim().is_empty())
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .collect();
                    if cells.iter().any(|c| !c.is_empty()) {
                        rows.push(cells);
                    }
                }
                if !rows.is_empty() {
                    blocks.push(Block::Table(rows));
                }
            }
            _ => {}
        }
        if blocks.len() >= MAX_BLOCKS {
            break;
        }
    }
    Ok(blocks)
}
/// 一个 `<w:p>` 的纯文本；`<w:tab/>` 变制表符，`<w:br/>` 变换行。
fn paragraph_text(node: roxmltree::Node) -> String {
    let mut out = String::new();
    for child in node.descendants().filter(|n| n.is_element()) {
        match local(&child).as_str() {
            "t" => {
                if let Some(text) = child.text() {
                    out.push_str(text);
                }
            }
            "tab" => out.push('\t'),
            "br" | "cr" => out.push('\n'),
            _ => {}
        }
    }
    out.trim_end().to_string()
}
/// 标题层级：`w:pStyle@w:val` 形如 Heading2，或 `w:outlineLvl@w:val`。
fn heading_level(node: roxmltree::Node) -> Option<usize> {
    for props in node.children().filter(|n| local(n) == "pPr") {
        for child in props.descendants() {
            match local(&child).as_str() {
                "pStyle" => {
                    let value = attribute(child, "val").unwrap_or_default();
                    if let Some(digit) = value.chars().find(|c| c.is_ascii_digit()) {
                        return digit.to_digit(10).map(|d| d as usize);
                    }
                    let lower = value.to_lowercase();
                    if lower.contains("title") {
                        return Some(1);
                    }
                    if lower.contains("heading") {
                        return Some(2);
                    }
                }
                "outlineLvl" => {
                    if let Some(level) = attribute(child, "val")
                        .and_then(|v| v.parse::<usize>().ok())
                        .map(|level| level + 1)
                    {
                        return Some(level);
                    }
                }
                _ => {}
            }
        }
    }
    None
}
/// `A1` → 0，`AA1` → 26。
fn column_index(reference: &str) -> usize {
    let mut index = 0usize;
    for ch in reference.chars() {
        if ch.is_ascii_alphabetic() {
            index = index * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
        } else {
            break;
        }
    }
    index.saturating_sub(1)
}
/// 0 → A，26 → AA。
fn column_name(mut index: usize) -> String {
    let mut name = String::new();
    index += 1;
    while index > 0 {
        let remainder = (index - 1) % 26;
        name.insert(0, (b'A' + remainder as u8) as char);
        index = (index - 1) / 26;
    }
    name
}
/// 只比较元素名，忽略命名空间前缀（w: / a: / r:）。
fn local(node: &roxmltree::Node) -> String {
    node.tag_name().name().to_string()
}
/// 按「本地名」取属性，兼容 `r:id`、`w:val` 这类带前缀的属性。
///
/// `<p:sldId id="256" r:id="rId3"/>` 同时有 `id` 和 `r:id`，两者的本地名都是 `id`；
/// 带命名空间的那个才是我们要的关系 id，所以优先返回它。
fn attribute<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    let mut fallback = None;
    for attribute in node.attributes() {
        if attribute.name() != name {
            continue;
        }
        if attribute.namespace().is_some() {
            return Some(attribute.value());
        }
        fallback = fallback.or(Some(attribute.value()));
    }
    fallback
}
/// 取指定子元素的文本。
fn child_text(node: roxmltree::Node, name: &str) -> Option<String> {
    node.children()
        .find(|n| local(n) == name)
        .and_then(|n| n.text())
        .map(|t| t.to_string())
}
/// 自然序比较，用于幻灯片文件名的兜底排序。
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    natord::compare(a, b)
}
/// HTML 转义：所有来自文档的文本都必须经过这里。
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_markup() {
        assert_eq!(escape("<b>a&b</b>"), "&lt;b&gt;a&amp;b&lt;/b&gt;");
    }
    #[test]
    fn columns_round_trip() {
        assert_eq!(column_index("A1"), 0);
        assert_eq!(column_index("B2"), 1);
        assert_eq!(column_index("AA9"), 26);
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(26), "AA");
    }
    #[test]
    fn normalizes_parts() {
        assert_eq!(
            normalize_part("xl", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            normalize_part("xl", "/xl/worksheets/sheet2.xml"),
            "xl/worksheets/sheet2.xml"
        );
        assert_eq!(
            normalize_part("ppt", "../slides/slide1.xml"),
            "slides/slide1.xml"
        );
    }
    #[test]
    fn document_blocks() {
        let xml = r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p>
            <w:p><w:r><w:t>正文</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>制表</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>要点</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
            <w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let blocks = parse_document(xml).unwrap();
        assert!(matches!(&blocks[0], Block::Heading(2, t) if t == "标题"));
        assert!(matches!(&blocks[1], Block::Paragraph(t) if t == "正文\t制表"));
        assert!(matches!(&blocks[2], Block::Bullet(t) if t == "要点"));
        match &blocks[3] {
            Block::Table(rows) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["A1".to_string(), "B1".to_string()]);
                assert_eq!(rows[1][0], "A2");
            }
            other => panic!("期望表格，得到 {other:?}", other = block_name(other)),
        }
    }
    fn block_name(block: &Block) -> &'static str {
        match block {
            Block::Heading(..) => "heading",
            Block::Paragraph(_) => "paragraph",
            Block::Bullet(_) => "bullet",
            Block::Table(_) => "table",
        }
    }
    #[test]
    fn sheet_cells() {
        let shared = vec!["共享一".to_string(), "共享二".to_string()];
        let xml = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1"><v>42</v></c></row>
            <row r="3"><c r="B3" t="inlineStr"><is><t>内联</t></is></c></row>
        </sheetData></worksheet>"#;
        let (rows, truncated) = parse_sheet(xml, &shared);
        assert!(!truncated);
        assert_eq!(rows.len(), 3, "第 2 行应保留为空行");
        assert_eq!(
            rows[0],
            vec![(0, "共享一".to_string()), (2, "42".to_string())]
        );
        assert!(rows[1].is_empty());
        assert_eq!(rows[2], vec![(1, "内联".to_string())]);
    }
    #[test]
    fn slide_text() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>标题</a:t></a:r></a:p>
            <a:p><a:r><a:t>要点一</a:t></a:r></a:p><a:p><a:r><a:t>要点二</a:t></a:r></a:p></p:sld>"#;
        let slide = parse_slide(xml);
        assert_eq!(slide.title, "标题");
        assert_eq!(
            slide.lines,
            vec!["要点一".to_string(), "要点二".to_string()]
        );
    }
    #[test]
    fn slide_order_from_rels() {
        let mut targets = HashMap::new();
        targets.insert("rId2".to_string(), "slides/slide1.xml".to_string());
        targets.insert("rId3".to_string(), "slides/slide2.xml".to_string());
        let xml = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst>
            <p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/>
        </p:sldIdLst></p:presentation>"#;
        let parts = slide_order(xml, &targets);
        assert_eq!(
            parts,
            vec!["ppt/slides/slide2.xml", "ppt/slides/slide1.xml"]
        );
    }
    #[test]
    fn sheet_names_keep_order() {
        let xml = r#"<workbook xmlns:r="r"><sheets>
            <sheet name="汇总" sheetId="1" r:id="rId1"/>
            <sheet name="明细" sheetId="2" r:id="rId2"/>
        </sheets></workbook>"#;
        let names = sheet_names(xml);
        assert_eq!(names[0].0, "汇总");
        assert_eq!(names[1].1, "rId2");
    }
}
