# Quick Look for Linux

[![CI](https://github.com/mingwangyang/quicklook_linux/actions/workflows/ci.yml/badge.svg)](./.github/workflows/ci.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Release](https://img.shields.io/badge/release-V0.0.10-green)](../../releases)

**English**: A native Quick Look–style file previewer for Linux, written in Rust + GTK4 — select a file in your file manager, press Space, and it pops up instantly.

当前版本：**V0.0.10**（`quicklook --version` 可查看）。

Rust 编写的 Linux 原生文件预览器，预览核心独立运行，适配 Linux Mint 22.3、niri / Wayland，并提供 Nemo、Dolphin、GNOME Files（Nautilus）、Thunar、Caja、Yazi 的可选入口。程序不依赖 niri，Wayland 与 X11 都能跑；实测过的桌面环境与验证边界见[适用环境](docs/USAGE.md#适用环境)。使用 GTK4、libmpv、Poppler、GtkSourceView、libvips 与 WebKitGTK，构建和运行均不需要 Wine。

已实现 P0/P1 预览路径、统一窗口交互、CLI/DBus、niri 配置和六种文件管理器适配（Nemo、Dolphin、Nautilus、Thunar、Caja、Yazi），常驻进程还带一个托盘图标。Office 文档（docx / xlsx / pptx）采用原生 OOXML 解析，压缩包支持逐层展开浏览包内文件，Markdown 里的 ```mermaid 流程图离线渲染，CSV / TSV 按分隔符渲染成表格（可切源码），JSON 默认打开源码视图。PSD 图层树、PCManFM 等其他文件管理器适配仍属于 P2。

## 目录

- [使用](#使用)
- [实测过的桌面环境](#实测过的桌面环境)
- [可选文件管理器入口](#可选文件管理器入口)
- [设置](#设置)
- [deb 安装](#deb-安装)
- [构建和安装](#构建和安装)
- [验证](#验证)
- [许可证](#许可证)
- [参与贡献](#参与贡献)

## 实测过的桌面环境

程序不依赖 niri，Wayland 与 X11 都能跑，差别只在窗口布局策略。各环境的实际验证状态（怎么测的、数据与截图见 [VALIDATION.md](docs/VALIDATION.md)）：

| 环境 | 验证状态 |
|---|---|
| **niri 26.04 + Wayland**（Linux Mint 22.3） | ✅ 主力验证环境：全功能真机验收，含窗口规则与 noctalia 面板托盘 |
| **X11 + KWin（KDE Plasma）** | ✅ 预览窗口置顶（0.0.9）真机实测；该会话下的文件管理器接入未测 |
| **Nemo 6.6 / Caja 1.26 / Thunar 4.18 / Nautilus 46.4 / Dolphin 23.08** | ✅ 各宿主的空格/右键链路在 Xephyr 隔离会话用真实进程实测（Dolphin 仅右键） |
| GNOME / KDE / Cinnamon / MATE / Xfce 普通桌面 | ⚠️ 代码路径已验证（无平铺规则、开箱即用），未逐桌面目测 |
| sway / Hyprland | ⚠️ 可识别并打印窗口规则片段供粘贴，未实机会话验证 |
| 托盘宿主 | ✅ noctalia 实测；⚠️ KDE 面板、GNOME AppIndicator、Waybar 未测 |

## 使用

完整的使用说明（安装、按键总表、各宿主差异、设置、配置项、排错）见 [**使用指南**](docs/USAGE.md)。下面是最常用的部分。

~~~bash
~/.niri-stack/prefix/bin/quicklook /path/to/file
~/.niri-stack/prefix/bin/quicklook --show /path/to/file
~/.niri-stack/prefix/bin/quicklook file1.png file2.pdf
~/.niri-stack/prefix/bin/quicklook --close
~~~

不传路径或使用 --pick 会打开应用自己的选文件窗口，无需先打开文件管理器。传路径时默认采用开关语义，--show 始终显示，多文件打开缩略图网格。

- 独立使用：从应用菜单启动 Quick Look，或按 niri 的 Ctrl+Alt+P，在原生选择窗口中选文件。
- Nemo（Mint 的“文件”应用）：空格预览第一个选中项，右键“用 Quick Look 预览”可看完整多选。
- Nemo 预览键：默认空格走原生 `org.nemo.Preview` 总线，不需要专用启动器；改成其它组合后由启动器加载按键旁路器，并屏蔽宿主自己的空格预览（需退出重开 Nemo）。
- 点击预览窗口后：Space/Esc 关闭，↑↓ 浏览，Enter 打开，P 固定，F 全屏（全屏时 Esc 先退出全屏，不关窗），I 属性，Ctrl+D 分离，Ctrl+C 复制文件或已选文本。
- 窗口分离（Ctrl+D）：当前预览转成独立进程窗口，可以平铺、长期开着，关掉它之前和之后都能继续用空格开新预览；独立窗口有自己的缓存与状态，互不影响。
- 图片：滚轮/捏合缩放，拖动平移，双击或 0 适应，1 原始像素，R/Shift+R 旋转。
- PDF：页码、Page Up/Down、T 缩略图、连续滚动；拖动选字，Shift+拖动平移。
- 媒体：K 暂停，逗号/句号逐帧，< > 倍速；音频支持封面/波形/频谱。
- 文本：W 换行，E 编码，分段继续加载。
- 外观：预览窗口、文档页面和设置窗口支持浅色 / 深色两套配色，默认浅色；在设置里切换后，之后打开的预览窗口使用新主题。
- 窗口布局：预览窗口默认浮动，可在设置里改成默认平铺（niri 窗口规则）；独立窗口（Ctrl+D）始终平铺，文件选择器与设置窗口始终浮动。
- 连续空格不再有固定等待：已删除 400 ms 冷却，按键去重使用按下/松开状态，不吞掉正常的下一次点击。

## 可选文件管理器入口

| 文件管理器 | 入口 | 说明 |
|---|---|---|
| Nemo | 空格、右键 | 预览键可改：默认空格走原生 `org.nemo.Preview`，改键后走启动器旁路器；右键动作保留完整多选 |
| Dolphin | 仅右键“用 Quick Look 预览”，**目前无法支持空格预览** | 多选；空格是宿主自己的打开/重命名行为，23.08 无预览热键接口，26.04+ 需 KIO ≥ 6.24 才能在宿主手动绑键 |
| GNOME Files / Nautilus | 原生空格；脚本菜单 | 原生接口传单文件，脚本入口传完整多选 |
| Thunar | 空格 / 右键“用 Quick Look 预览” | 空格靠 LD_PRELOAD 适配器接管；旧实例（未加载、旧版或重装后残留映射）由启动器询问重启；右键 `%F` 传完整多选 |
| Caja | 空格 / 右键“用 Quick Look 预览” | 用户扩展加载 Rust 适配；安装 python3-caja 后，终端直接运行 caja 也支持空格。没有该扩展时可用显式适配启动器 |
| Yazi | 空格、Esc、光标跟随 | 默认绑定空格，Esc 关闭且保留原有 Esc |

预览键由同一份配置决定，默认空格，但不是每个宿主都能完全跟随：Nemo / Thunar / Caja / Yazi 改键后生效（Nemo 改键后会关掉它自己的空格预览，需重启；从终端直接启动的 Nemo 不经过启动器，仍用空格）；Nautilus 的空格由宿主自己处理，只有带 Ctrl/Alt 的组合键能写进脚本加速键；**Dolphin 目前无法支持空格预览**：按空格是宿主自己的打开/重命名行为，本项目没有可用的接管接口（23.08 无预览热键接口；26.04+ 需 KIO ≥ 6.24 才能在宿主手动绑定，已核对官方源码但未真机验收）。改键后都要重启对应的文件管理器。

## 设置

~~~bash
quicklook --settings          # 打开设置窗口
quicklook --apply-hosts       # 按当前配置重写各文件管理器集成，不开窗口
~~~

设置窗口可改主题、浮动/平铺布局、全局和各宿主的预览键、缓存与文件管理器选择。「保存设置」只保存选项；文件管理器接入需单独点击「应用文件管理器接入 / 注入」。新配置默认不接管按键，已有配置保留原选择。「撤销全部接入」移除托管文件/块并恢复备份的宿主入口，不关闭正在浏览的窗口。

改键的生效范围：默认所有宿主跟随「全局默认」，也可以给单个文件管理器指定别的键（例如 Nemo 用空格、Yazi 用 F6）。Nemo、Thunar 使用适配启动器，Caja 可由用户扩展加载同一个 Rust 按键旁路器（按键在进程启动时读一次，改完要重启该文件管理器），Yazi 改写 keymap，Nautilus 写脚本加速键。把某个宿主的裸空格改成别的键时，该宿主里原来的空格会被旁路器吃掉，避免同时弹出两套预览。Nautilus 的裸空格走原生预览协议，Ctrl/Alt 组合会写进脚本加速键；Dolphin 23.08 没有空格预览入口，只能右键。

所有入口都调用同一 Rust 预览服务，任意一种文件管理器均非必装依赖。详见 [兼容方式与验证范围](docs/FILE_MANAGERS.md)。

常驻的 `quicklookd` 还会注册一个 StatusNotifierItem 托盘图标（应用自己的蓝底「文档 + 放大镜」图标，浅色与深色主题下一致），只有在宿主提供托盘区时才显示：

~~~text
左键     打开选文件预览
右键菜单  设置… / 重启 / 退出
~~~

托盘菜单的“重启”会让常驻服务先拉起新进程，等旧进程释放 D-Bus 名后再退出，因此不需要手动结束进程；文件管理器窗口不受影响，正在显示的预览会关闭，下次打开时使用新配置。

托盘只是入口之一，关掉它不影响任何预览功能；如果宿主没有 StatusNotifierWatcher，`quicklookd` 会在启动日志里说明并继续提供 DBus 服务。

想让托盘图标一登录就在，可以在设置窗口的「启动」里打开「登录时自动启动常驻服务」（默认关闭）。它写的是一个标准的 XDG autostart 条目 `~/.config/autostart/org.quicklook.Preview.desktop`，只启动 `quicklookd`，不会把预览窗口也拉起来；桌面环境自带的启动项管理工具里也能看到并停用同一个条目。

## deb 安装

Ubuntu 24.04 / Linux Mint 22.x amd64：

```bash
sudo apt install ./dist/quicklook_V0.0.10_amd64.deb
```

应用菜单有独立的「Quick Look 设置」入口。Caja 安装 python3-caja、启用接管并应用后，可直接在终端运行 `caja`；Thunar 等启动器适配方式见 FILE_MANAGERS.md。不会替换终端里的同名命令。已运行的旧实例需正常退出并重新打开。

打包、迁移与卸载见 [deb 安装说明](docs/PACKAGING.md)，本次检查与修复见 [代码检查记录](docs/REVIEW.md)。

## 构建和安装

完整的环境搭建说明（系统依赖清单与最小编译集、Rust 工具链调整、其他发行版包名对照、NixOS/容器、常见问题）见 [**开发环境搭建**](docs/DEVELOPMENT.md)。最快路径：

~~~bash
bash scripts/install-system-deps.sh --install   # Ubuntu/Mint；先 --dry-run 可预览
bash scripts/install-rust.sh && source "$HOME/.cargo/env"
cargo build --locked --release --workspace
python3 scripts/install-local.py --with-niri --with-file-managers
~~~

安装到用户目录（默认 `~/.niri-stack/prefix`，可用 `QUICKLOOK_PREFIX` 改），无需 sudo；niri 配置先验证后写入，变更及备份记录在 .local/dev-environment/local-install-*.json。偏好系统包管理器可用 deb 路线，见 [deb 安装](#deb-安装)。

[**使用指南**](docs/USAGE.md) · [交接说明](docs/HANDOVER.md) · [环境与构建步骤](docs/DEVELOPMENT.md) · [执行记录](docs/SETUP_LOG.md) · [使用、边界和插件](docs/IMPLEMENTATION.md) · [验证记录](docs/VALIDATION.md) · [可行性分析](docs/FEASIBILITY.md) · [原始需求](REQUIREMENTS.md)

## 验证

~~~bash
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/smoke-test.py --profile release
python3 scripts/extra-fixtures.py --download-samples
python3 scripts/smoke-test.py --profile release
python3 scripts/test-nemo-adapter.py --profile release
python3 scripts/robustness-test.py
python3 scripts/benchmark.py --profile release
~~~

样本和截图只保存在忽略的 .local/。桌面锁定可加 --no-screenshots。测试会切换本项目服务，Nemo 测试使用隔离会话。

本机最新一轮测量：预览进程首次启动约 341ms，短时间关闭重开中位约 19ms，缓存切换中位约 18ms，万行文本首段约 34ms，关闭后的常驻进程约 6.5MiB。**这是请求到数据/控件准备就绪的时间，包含 CLI 与状态查询，不是合成器首帧测量，也不是所有格式的保证。** 早先冷启动样本为 291ms 和 441ms，缓存切换最大值为 50ms，尚不能声称全部性能硬指标通过。108MP PNG 首轮约 1.4–2 秒，放大后按视口补充细节。

## 许可证

本项目以 [GPL-3.0-or-later](LICENSE) 发布，二次分发请保留版权与许可声明。`integration/vendor/` 内置的 mermaid.min.js 是 MIT 许可的第三方资源，见 [vendor 资源说明](integration/vendor/README.md)。

## 参与贡献

- 发现问题：[Bug 报告](../../issues/new?template=bug_report.yml)；功能建议：[功能建议](../../issues/new?template=feature_request.yml)
- 安全漏洞请勿公开提交，走 [SECURITY.md](SECURITY.md) 的私下渠道
- 代码贡献流程、质量门与分支模型见 [CONTRIBUTING.md](CONTRIBUTING.md)
- 版本历史见 [CHANGELOG.md](CHANGELOG.md)
