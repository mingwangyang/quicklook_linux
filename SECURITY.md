# 安全策略

## 报告漏洞

Quick Look 直接解析来自文件系统的不可信内容（图片、PDF、Office、压缩包、Markdown/HTML 等），
我们认真对待这类攻击面。

**请勿通过公开 Issue 报告安全漏洞。** 请发送邮件至 **quicklook.sec@example.org**，邮件中请尽量包含：

- 受影响版本（`quicklook --version` 输出或提交哈希）
- 复现步骤，以及能触发问题的样本文件（如无法分享，请描述构造方式）
- 影响评估（信息泄露 / 代码执行 / 沙箱逃逸 / DoS 等）
- 你已知的缓解措施

收到报告后目标在 72 小时内确认，并在修复发布前与报告者同步进展。修复会在
[CHANGELOG.md](CHANGELOG.md) 中标注安全性质。

## 支持的版本

| 版本 | 支持状态 |
|---|---|
| 0.0.9（当前 main） | ✅ 接受安全报告 |
| 更早版本 | 仅在最新版可复现时修复 |

## 攻击面说明（供参考）

- 预览渲染后端直接接触文件字节流：libvips / Poppler / libmpv / WebKitGTK / libarchive 等。
- WebKit 视图默认禁用 JavaScript；仅当 Markdown 含 mermaid 代码块时注入内置的 mermaid.min.js。
- 压缩包预览限制解压层级并防御路径穿越（zip-slip）。
- 文件管理器接入（LD_PRELOAD 适配器、桌面文件、脚本）只操作本项目自己的配置与托管文件。
