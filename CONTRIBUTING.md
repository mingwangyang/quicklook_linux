# 贡献指南

感谢关注 Quick Look for Linux！欢迎通过 Issue 反馈问题、通过 Pull Request 提交代码。

## 分支模型

- `main`：稳定发布线。只接受来自 `dev` 的合并，每次对外发布在此打版本标签（`V0.0.9`、`V0.1.0` …）。
- `dev`：日常开发线。功能开发请基于 `dev` 拉出功能分支（如 `feat/pdf-annotate`），完成后合回 `dev`。
- 发布节奏：阶段性稳定后，把 `dev` 合入 `main` 并打标签。

## 开发环境

目标环境是 Linux Mint 22.3 / Ubuntu 24.04（Wayland 优先，X11 可用）。其他发行版可以构建，但不在验证范围。

```bash
# 1. 系统依赖（先 --dry-run 预览）
bash scripts/install-system-deps.sh --install

# 2. Rust 工具链（rust-toolchain.toml 指定 1.99.1）
bash scripts/install-rust.sh

# 3. 验证 GTK4/Wayland 工具链可用
bash scripts/verify-rust-gtk.sh --basic

# 4. 构建
cargo build --locked --release --workspace
```

构建与安装的完整说明见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 提交前必须通过的质量门

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

涉及渲染、宿主接入或性能的改动，还请尽量补跑对应脚本（`scripts/smoke-test.py`、
`scripts/test-nemo-adapter.py` 等，见 [README 的验证一节](README.md#验证)）。CI 在
Ubuntu 24.04 上运行同一组质量门。

## 提交与 PR 约定

- 提交信息用 `type: 摘要` 格式（`feat:` / `fix:` / `docs:` / `chore:` / `build:` / `refactor:` / `test:`），摘要首行不超过 72 字符，正文说明动机。
- 一个 PR 聚焦一件事；UI 改动请附截图或录屏。
- 新增依赖需要在 PR 描述里说明理由（本项目对原生依赖比较克制）。
- 合并策略：`dev` ← 功能分支用 squash 或 merge 均可；`main` ← `dev` 保持线性（`--ff-only`）。
- 新增面向用户的行为请同步更新 [README.md](README.md) / [docs/USAGE.md](docs/USAGE.md)，并在 [CHANGELOG.md](CHANGELOG.md) 的 Unreleased 段落记一笔。

## 报告问题

- 功能异常：用 [Bug 报告](.github/ISSUE_TEMPLATE/bug_report.md)模板，附发行版、桌面环境、文件管理器与复现步骤。
- 安全漏洞：**请勿公开提交**，走 [SECURITY.md](SECURITY.md) 的私下报告渠道。

## 许可

提交即表示你同意以 [GPL-3.0-or-later](LICENSE) 许可你的贡献。内置第三方资源见
[integration/vendor/README.md](integration/vendor/README.md)，新增第三方资源请同样登记来源与许可。
