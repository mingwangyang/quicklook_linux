# 内置第三方资源

## mermaid.min.js.gz

- 上游：<https://github.com/mermaid-js/mermaid>
- 版本：11.17.2（`https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js`）
- 许可：MIT
- 未压缩大小：3 572 661 字节；`gzip -9` 后：约 953 KiB
- 未压缩 SHA-256：`581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8`
- 用途：Markdown 里的 ```mermaid 流程图渲染，由 `crates/daemon/src/mermaid.rs` 在运行时解压并通过 WebKit 用户脚本注入。
- 更新方式：下载同名 dist 文件后 `gzip -9 -c mermaid.min.js > integration/vendor/mermaid.min.js.gz`，同步更新本文件的版本号与 SHA-256，然后重跑 `cargo test -p quicklook-viewer`。

以 gzip 形式入库而不是原文件，是因为它会被 `include_bytes!` 编进 `quicklook-viewer`：未压缩内嵌会让二进制多 3.4 MiB，压缩后只多约 0.95 MiB，运行时首次遇到流程图时解压一次并缓存。

mermaid 只在**含 mermaid 代码块的 Markdown**里启用：这些视图才打开 WebKit 的 JavaScript 并注入该脚本，其余视图仍然关闭 JavaScript。页面 CSP 不含 `script-src`（沿用 `default-src 'none'`），所以 Markdown 正文里手写的 `<script>` 不会执行。
