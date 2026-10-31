//! Markdown 里的 Mermaid 流程图。
//!
//! 预览用的 WebView 默认关闭 JavaScript（见 `ui::views::web_view`），Markdown 正文里的
//! ```mermaid 代码块原来只会以源码形式显示。要画图有两个选择：在 Rust 里自己实现布局，
//! 或者让 Mermaid 在页面里跑。
//!
//! 这里选后者，但把风险面收窄到最小：
//!
//! - 只有**确实含 mermaid 代码块**的 Markdown 视图才打开 JavaScript，其它视图照旧关闭；
//! - Mermaid 由 WebKit 用户脚本注入（`UserScript`），不来自文档内容；
//! - 页面 CSP 保持 `default-src 'none'`，不含 `script-src`，所以 Markdown 正文里手写的
//!   `<script>` 仍然不会执行——CSP 只约束文档自身的脚本，用户脚本由引擎注入，不受这条限制；
//! - 代码块的文字用 `textContent` 取出再交给 Mermaid，不经过 HTML 解析；
//! - Mermaid 用 `securityLevel: 'strict'`，标签里的 HTML 会被过滤。
//!
//! Mermaid 本体以 gzip 形式内嵌（见 `integration/vendor/README.md`），首次使用时解压一次
//! 并缓存在进程内，避免每个文档都解压一遍。

use std::io::Read;
use std::sync::OnceLock;

/// mermaid 11.17.2 的 UMD 构建，`gzip -9` 压缩后入库（MIT 许可）。
const BUNDLE: &[u8] = include_bytes!("../../../integration/vendor/mermaid.min.js.gz");

/// 脚本消息处理器的名字，Rust 侧用同名注册。
pub const HANDLER: &str = "quicklookMermaid";

/// 解压后的 Mermaid 脚本；解压失败时返回空串，调用方据此退回纯代码块显示。
pub fn bundle() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut text = String::new();
        match flate2::read::GzDecoder::new(BUNDLE).read_to_string(&mut text) {
            Ok(_) => text,
            Err(error) => {
                eprintln!("quicklook: 内置 Mermaid 解压失败，流程图将只显示源码：{error}");
                String::new()
            }
        }
    })
}

/// Markdown 源码里是否有 ```mermaid 代码块。
///
/// 只做一个方向上的宽松判断：宁可多认（把普通视图也打开 JavaScript），不要漏认，
/// 因为漏判的表现就是「图不显示」，而多判只是多付一点启动开销。
pub fn has_diagram(source: &str) -> bool {
    let mut fence: Option<(char, usize)> = None;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some((marker, count)) = opening_fence(trimmed) else {
            continue;
        };
        match fence {
            None => {
                if is_mermaid(trimmed, count) {
                    return true;
                }
                fence = Some((marker, count));
            }
            Some((open_marker, open_count)) => {
                if marker == open_marker
                    && count >= open_count
                    && trimmed[count..].trim().is_empty()
                {
                    fence = None;
                }
            }
        }
    }
    false
}

/// 行首的围栏标记（``` 或 ~~~，三个以上）及其长度。
fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|c| *c == marker).count();
    (count >= 3).then_some((marker, count))
}

/// 围栏后的信息串是否为 mermaid。
fn is_mermaid(trimmed: &str, count: usize) -> bool {
    trimmed[count..]
        .split_whitespace()
        .next()
        .map(|info| info.eq_ignore_ascii_case("mermaid"))
        .unwrap_or(false)
}

/// 注入到页面末尾的引导脚本：把代码块换成 Mermaid 容器并渲染。
///
/// 结果通过 `window.webkit.messageHandlers` 回报，Rust 侧据此更新状态与日志。
pub fn bootstrap(dark: bool) -> String {
    let theme = if dark { "dark" } else { "default" };
    // 连线标签（mermaid 的 edgeLabel）默认带一块不透明底色，深浅主题下都要显式对齐，
    // 否则深色主题里会出现浅色小方块。
    let surface = if dark { "#11151b" } else { "#ffffff" };
    format!(
        r#"(function () {{
  var post = function (text) {{
    try {{ window.webkit.messageHandlers.{HANDLER}.postMessage(String(text)); }} catch (error) {{}}
  }};
  window.addEventListener('error', function (event) {{
    post('error:' + (event.message || '脚本错误'));
  }}, true);
  var codes = document.querySelectorAll('pre > code[class*="language-mermaid"], pre > code[class*="lang-mermaid"]');
  var nodes = [];
  for (var i = 0; i < codes.length; i += 1) {{
    var code = codes[i];
    var pre = code.parentNode;
    if (!pre || !pre.parentNode) {{ continue; }}
    var box = document.createElement('div');
    box.className = 'mermaid';
    box.textContent = code.textContent;
    pre.parentNode.replaceChild(box, pre);
    nodes.push(box);
  }}
  if (!nodes.length) {{ post('none'); return; }}
  if (typeof mermaid === 'undefined') {{ post('unavailable'); return; }}
  try {{
    mermaid.initialize({{
      startOnLoad: false,
      securityLevel: 'strict',
      theme: '{theme}',
      fontFamily: 'inherit',
      themeVariables: {{ edgeLabelBackground: '{surface}' }},
      flowchart: {{ useMaxWidth: true }},
      sequence: {{ useMaxWidth: true }}
    }});
    var run = mermaid.run({{ nodes: nodes, suppressErrors: true }});
    if (run && typeof run.then === 'function') {{
      run.then(function () {{ post('ok:' + nodes.length); }},
               function (error) {{ post('error:' + (error && error.message ? error.message : error)); }});
    }} else {{
      post('ok:' + nodes.length);
    }}
  }} catch (error) {{
    post('error:' + (error && error.message ? error.message : error));
  }}
}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fenced_flowchart() {
        assert!(has_diagram("前\n```mermaid\ngraph TD\nA-->B\n```\n"));
        assert!(has_diagram("```Mermaid\ngraph TD\n```"));
        assert!(has_diagram("~~~ mermaid\ngraph LR\n~~~"));
        assert!(has_diagram("```  mermaid  extra\ngraph TD\n```"));
    }

    #[test]
    fn ignores_other_blocks() {
        assert!(!has_diagram("# 标题\n\n普通段落\n"));
        assert!(!has_diagram("```rust\nfn main() {}\n```\n"));
        assert!(!has_diagram("```\ngraph TD\nA-->B\n```\n"));
        // 信息串只是以 mermaid 开头不算
        assert!(!has_diagram("```mermaidx\ngraph TD\n```"));
        assert!(!has_diagram("```js-mermaid\ngraph TD\n```"));
    }

    #[test]
    fn closing_fence_must_match_length() {
        // 四反引号开、三反引号不能关
        let text = "````\n```mermaid\n````\n";
        assert!(!has_diagram(text));
    }

    #[test]
    fn bootstrap_carries_theme_and_handler() {
        assert!(bootstrap(true).contains("theme: 'dark'"));
        assert!(bootstrap(false).contains("theme: 'default'"));
        assert!(bootstrap(false).contains(HANDLER));
        assert!(bootstrap(false).contains("language-mermaid"));
        assert!(bootstrap(false).contains("securityLevel: 'strict'"));
        // 连线标签底色必须跟主题一起切换，否则深色下会出现浅色小块
        assert!(bootstrap(true).contains("edgeLabelBackground: '#11151b'"));
        assert!(bootstrap(false).contains("edgeLabelBackground: '#ffffff'"));
    }

    #[test]
    fn bundle_decompresses_to_the_bundle() {
        let text = bundle();
        assert!(
            text.len() > 1_000_000,
            "解压后的 mermaid 过小：{}",
            text.len()
        );
        assert!(
            text.contains("globalThis[\"mermaid\"]"),
            "不是预期的 UMD 构建"
        );
        // 第二次取用命中的是缓存的同一份
        assert!(std::ptr::eq(bundle(), text));
    }
}
