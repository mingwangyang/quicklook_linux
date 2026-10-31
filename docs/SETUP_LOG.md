# 开发环境安装记录

日期：2026-10-25，时间均为 Asia/Shanghai。工作目录：`~/work/code/self/quick_look_linux`。

## 00:52 — 盘点

- Linux Mint 22.3 / x86_64；niri 26.04 / Wayland；Nemo 6.6.3。
- 已有 GTK4 4.14.5、GLib/GIO 2.80.0、epoxy 1.5.10、Wayland 1.22.0、libjxl 0.7.0 开发库及 C/C++、Meson、Ninja、CMake。
- Rust/rustup/Cargo 未安装；GtkSourceView、Poppler-GLib、libmpv、libarchive、WebKitGTK 6.0、libheif、LibRaw、libvips 等开发库缺失。
- `sudo -n -v` 返回需要密码；系统安装需用户在终端完成，不索取或保存密码。

## 00:53 — 官方源与 GPU 检查

- `https://sh.rustup.rs`、`https://static.rust-lang.org/dist/channel-rust-stable.toml` 可访问。
- 官方 stable manifest 日期 2026-10-15，Rust 1.99.1，x86_64-unknown-linux-gnu 可用；工程已固定该版本。
- 下载的 rustup 引导脚本 SHA-256：`9502b514362a21096210d498f143b8cdacda365d3dfcf45e8f5541e089ccafc0`。
- `vainfo --display wayland` 成功：Mesa radeonsi / AMD Renoir，VA-API 1.20；报告 H.264、HEVC Main/Main10、VP9 等解码 profile。此结果证明驱动接口可用，不代表已完成 libmpv 播放验证。

## 00:54 — 系统安装预检与修正

- 首次模拟安装发现 apt 默认选中 JACK1 的开发包，会移除 JACK2 及 Wine/Winetricks；未执行实际系统安装。
- 清单显式增加 `libjack-jackd2-dev`，保留已有 JACK2；脚本增加 `--no-remove`。
- 修正后模拟结果：**0 升级，121 新增，0 移除，2 未升级**。
- 安全方案日志：`.local/dev-environment/system-deps-20261025T005456+0800.log`。
- 完整安装命令及包清单见 [开发环境搭建](DEVELOPMENT.md)。

## 00:55 — Rust 安装完成

- 执行：`bash scripts/install-rust.sh`。
- 参数：普通用户；Rust 1.99.1；minimal；rustfmt + clippy；`--no-modify-path`。
- 引导脚本 SHA-256 校验通过，开始下载官方安装器。
- 日志：`.local/dev-environment/rust-install-20261025T005501+0800.log`。
- 00:55:44 完成，退出码 0。
- rustup 1.29.2；rustc 1.99.1（2026-10-13，LLVM 23.1.1）；Cargo 1.99.1；rustfmt 1.10.0-stable；clippy 0.1.99。
- 安装位置：`~/.cargo`、`~/.rustup`；shell 配置文件未改，当前终端可 `source "$HOME/.cargo/env"` 启用。

## 00:58 — Rust / GTK4 编译验证通过

- 增加独立验证程序 `tools/environment-check/`，不包含产品功能。
- 固定 gtk4 0.11.5 / sourceview5 0.11.2 / webkit6 0.6.1；核对 GTK/GLib/GIO 依赖代际一致，生成 Cargo.lock。
- 执行：`bash scripts/verify-rust-gtk.sh --basic`，使用 `GDK_BACKEND=wayland`；构建输出写入 `target/environment-check/`。
- 初次环境检查确认 Rust 工具可用，尚缺 9 项原生开发库模块及相关系统包；等待系统安装后复查。
- 00:58:56 编译运行成功，退出码 0；首次编译用时 54.34 秒。
- 运行结果：GTK 4.14.5；显示类型 `GdkWaylandDisplay`；会话 DBus 连接成功。
- 日志：`.local/dev-environment/rust-gtk-20261025T005802+0800.log`。
- Rust 包下载、编译器、系统链接器、GTK4 绑定及原生 Wayland 初始化已验证；尚未执行实际媒体播放或窗口交互验证。
- 随后运行 `cargo fmt --manifest-path tools/environment-check/Cargo.toml` 和 `CARGO_TARGET_DIR=target/environment-check cargo clippy --locked --manifest-path tools/environment-check/Cargo.toml --no-deps -- -D warnings`，基础配置检查通过，无警告。

## 01:02 — 用户完成系统依赖安装

- 用户执行 `bash scripts/install-system-deps.sh --install`，01:02:47 成功，退出码 0。
- 实际安装：121 个新增包、0 升级、0 移除；下载 44.9MB，新增空间约 215MB。
- 日志：`.local/dev-environment/system-deps-20261025T010159+0800.log`，末尾包含所有请求包的实际版本。
- 此前一次运行在 apt 确认时中止，日志保留为 `system-deps-20261025T010140+0800.log`；未进行安装。
- 复查 JACK2 的 amd64/i386 库、Wine 和 Winetricks 仍处于已安装状态。

主要开发包版本：

| 包 | 版本 |
|---|---|
| libgtk-4-dev | 4.14.5+ds-0ubuntu0.10 |
| libgtksourceview-5-dev | 5.12.0-1build1 |
| libpoppler-glib-dev | 24.02.0-1ubuntu9.11 |
| libmpv-dev | 0.37.0-1ubuntu4 |
| libarchive-dev | 3.7.2-2ubuntu0.9 |
| libwebkitgtk-6.0-dev | 2.52.6-0ubuntu0.24.04.1 |
| libheif-dev | 1.17.6-1ubuntu4.9 |
| libraw-dev | 0.21.2-2.1ubuntu0.24.04.2 |
| libjxl-dev | 0.7.0-10.2ubuntu6.1 |
| libvips-dev | 8.15.1-1.1build4 |
| libva-dev | 2.20.0-2ubuntu0.2 |
| libjack-jackd2-dev | 1.9.21~dfsg-3ubuntu3 |

注意：`pkg-config mpv` 的 2.2.0 是 libmpv 客户端 API 版本，播放器包版本为 0.37.0。

## 01:05 — 全量环境检查通过

- 执行 `bash scripts/check-environment.sh --gpu`；工具、固定 Rust 版本、全部开发库、原生架构 apt 包、Wayland 和 VA-API 检查均通过，缺失/失败项 **0**。
- 日志：`.local/dev-environment/check-20261025T010519+0800.log`。
- 修复检查脚本的 multiarch 误报：同名 amd64/i386 包的状态不能直接拼接比较，现明确检查本机架构或 `Architecture: all`。无需额外安装包。

## 01:06 — 完整 Rust 编译和后端链接验证通过

- 执行 `bash scripts/verify-rust-gtk.sh --full`；01:06:15 成功，退出码 0，首次完整编译用时 1 分 17 秒。
- 日志：`.local/dev-environment/rust-gtk-20261025T010457+0800.log`。
- 原生 Wayland 显示为 `GdkWaylandDisplay`，会话 DBus 连接正常。
- GtkSourceView 成功找到 Rust 语法定义；WebKit 注册类型为 `WebKitWebView`。
- 实际调用各后端版本接口：Poppler 24.02.0、libarchive 3.7.2、libheif 1.17.6、LibRaw 0.21.2、libvips 8.15.1、libmpv API 2.2、JPEG XL 0.7.0，全部成功。
- Rust 格式检查、脚本语法检查、文档链接检查与 `git diff --check` 通过。

**环境搭建完成。** 已具备 P0/P1 开发所需的工具链、原生开发库和可重复运行的验证入口。当前验证覆盖编译、链接、会话连接及库初始化/版本接口，不代表已经实现预览功能，也不替代后续 libmpv 实际播放、PDF 渲染、Nemo 接入及性能原型验收。

## P0/P1 开发与本机集成

- 新增 Cargo 工作区：core、CLI/轻量 broker、GTK4 viewer、Rust Nemo 适配。所有产品代码均为 Linux 原生，无 Windows 目标、Wine 或兼容层依赖。
- 执行 source ~/.cargo/env 后运行 cargo build --locked --release --workspace，用户目录内编译，无新增系统包。运行格式检查、工作区测试与 Clippy，结果见 VALIDATION.md。
- 执行 python3 scripts/install-local.py --with-niri，将发布程序安装进 ~/.niri-stack/prefix/bin，适配库放 prefix/lib/quicklook；用户 DBus 激活、桌面入口、Nemo 右键动作放 ~/.local/share，niri include/规则在 ~/.config/niri。
- 安装器每次记录文件清单及备份路径到 .local/dev-environment/local-install-*.json；最近一次安装时间和实际版本由该日志与已安装程序校验结果确定。开发阶段曾安装 debug，最终更新为 release。
- 没有自动重启用户 Nemo。启用空格联动需要用户退出“文件”应用后使用“文件（Quick Look）”入口重新启动。
- 隔离 Nemo 测试首次激活了共用路径的 AT-SPI 辅助服务，造成当前会话无障碍 socket 冲突。已针对确切的当前会话辅助进程恢复 socket，并复查可用；测试脚本改为禁用私有测试会话的无障碍桥接及远端卷监视，避免复现冲突。用户文件管理器进程未被终止。
- 锁屏期间只运行后台验证，未解锁或绕过锁屏。随后通过本应用自己的窗口导出完成原生画面检查，未保存桌面或其他应用画面。
- 详细功能覆盖、性能测量和未完成的人工/首帧验收见 VALIDATION.md，不把编译或数据就绪当作所有需求指标已验收。

## 独立入口与四种文件管理器适配

- 按追加需求提供 Nemo、Dolphin、GNOME Files（Nautilus）、Yazi 四种可选适配，核心不要求任何文件管理器运行或安装。
- 新增 GTK4 原生多选窗口：直接启动应用、无参数 quicklook、--pick、niri Ctrl+Alt+P 均可打开。选择窗口不会被 3 秒后台回收计时误关闭。
- 新增 Rust Nautilus Previewer2 桥接，兼容三/四参数接口及 Visible 属性；完整多选另经脚本 URI 入口。Dolphin 采用 KIO 服务菜单，Yazi 采用官方 Lua 插件；不为兼容安装 Qt 或 Nautilus 系统包。
- 新增独立 Rust Wayland 文件剪贴板持有程序，GNOME/KDE MIME 并存。增加 Cargo 依赖 wl-clipboard-rs 0.9.4，无新增 apt 安装。
- 执行 cargo build --locked --release --workspace 和 python3 scripts/install-local.py --with-niri --with-file-managers。实际文件及备份清单见最近 local-install-*.json，记录包含安装选项与 Yazi 按键。
- Yazi 默认 Ctrl+Space 预览，保留默认空格多选；已有自定义配置保留，行内数组等不能安全追加的情况生成合并片段并提示。Nautilus 已有 Ctrl/Primary+Space 不覆盖。所有宿主均未强制重启。
- 验证包括 14 项 Rust 测试、Clippy、既有格式烟测、Nemo 隔离宿主测试，以及新的 GNOME 协议、Dolphin 服务菜单派发、Yazi 真实 PTY、配置保护和独立选择器测试。宿主覆盖与剩余边界见 FILE_MANAGERS.md。
- 本轮桌面未提供预览窗口的绘制帧，截图导出返回“窗口尚未完成绘制”；未解锁或绕过桌面锁定，格式回归改用 --no-screenshots。程序化验证不代替缺失的 GNOME 宿主 UI 实测。

## 2026-10-25：检查修复与 deb 构建步骤

本轮没有新增 apt 软件包。已存在 dpkg-dev、binutils、fakeroot、desktop-file-utils，直接使用。Rust 沿用已记录的工具链。

执行步骤：

```bash
source "$HOME/.cargo/env"
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --workspace
python3 scripts/build-deb.py --no-build
python3 scripts/test-deb.py dist/quicklook_0.1.0-1_amd64.deb
python3 scripts/test-local-install.py
```

本机源码版更新：

```bash
env -u VIPSHOME -u GDK_BACKEND python3 scripts/install-local.py --with-file-managers --with-niri
```

安装位置仍是 ~/.niri-stack/prefix，保留原配置选择；运行时生成的接入启动器迁往 ~/.local/share/quicklook/launchers。清理了此前同名 PATH 软链接 ~/.local/bin/caja 和 thunar，仅清理指向本工具的链接。没有重启文件管理器。安装记录位于 .local/dev-environment/local-install-20261025T232612+0800.json 及之后的记录。

当前仅生成并隔离测试了 deb，没有在宿主执行 sudo 安装。交付给用户的 deb 安装、源码版迁移和卸载步骤见 [PACKAGING.md](PACKAGING.md)。

## 2026-10-26：空格修复与补测

没有新增系统安装包。为了补测 GNOME Files，apt-get download 下载了 nautilus、nautilus-data、libportal1、libportal-gtk4-1、libtracker-sparql-3.0-0、libunity9、libunity-protocol-private0、libdee-1.0-4、libunity-scopes-json-def-desktop，dpkg-deb -x 解到 .local/space-hosts/nautilus；glib-compile-schemas 编译该临时目录的 schema。使用已存在的 bwrap 仅为测试提供只读库路径映射，没有 sudo、没有写 /usr。

```bash
source "$HOME/.cargo/env"
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --workspace
python3 scripts/test-space-keys.py --profile release --nautilus .local/space-hosts/nautilus
python3 scripts/test-caja-adapter.py --profile release
python3 scripts/build-deb.py --no-build
python3 scripts/test-deb.py dist/quicklook_0.1.0-2_amd64.deb
```

现有源码版继续通过 install-local.py --with-file-managers --with-niri 更新。不会重启用户的文件管理器；旧进程中的适配器需用户正常退出重开才更新。新的 deb 修订号为 0.1.0-2，便于覆盖安装旧 0.1.0-1。

## 2026-10-26：版本统一为 V0.0.1

按用户指定统一版本：Cargo/锁文件、命令行 --version、设置标题、运行状态、构建元数据和安装记录。deb 版本字段为 0.0.1，包名 quicklook_V0.0.1_amd64.deb。未新增系统依赖。构建脚本会校验二进制版本，避免 --no-build 把旧程序误标成新版。

## 2026-10-26：登录后的 Thunar 接入修复

未安装新系统依赖。修复前已确认：原 niri Mod+E 脚本直接执行 thunar，绕过菜单覆盖；服务已自动启动。使用现有工具链执行：

```bash
source "$HOME/.cargo/env"
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --workspace
python3 scripts/test-niri-launcher.py
python3 scripts/test-local-install.py
env -u VIPSHOME -u GDK_BACKEND python3 scripts/install-local.py --with-file-managers --with-niri
python3 scripts/test-space-keys.py --hosts thunar --thunar-entry launcher --prefix "$HOME/.niri-stack/prefix"
python3 scripts/test-space-keys.py --hosts thunar --thunar-entry desktop --prefix "$HOME/.niri-stack/prefix"
python3 scripts/test-space-keys.py --hosts thunar --thunar-entry dbus --prefix "$HOME/.niri-stack/prefix"
python3 scripts/build-deb.py --no-build --version 0.0.1-1
python3 scripts/test-deb.py dist/quicklook_V0.0.1-1_amd64.deb
```

安装记录见 .local/dev-environment/local-install-20261026T093225+0800.json 及之后记录。修改 prefix/bin/launch-file-manager 前按事务备份；新增启动块带原命令回退，卸载可移除。应用接入生成用户级 Thunar 专用 D-Bus 服务文件与 thunar.service 配置片段，并只重新读取服务定义。没有重启用户 Thunar，没有改变默认文件夹关联。用户已反馈恢复正常。应用版本仍为 V0.0.1，deb 修订号为 0.0.1-1，已发布标签保持原样。

## 2026-10-26：Caja 普通命令启动修复

已有 python3-caja 1.26.0-1build5 与 Caja 1.26.3+mint1，无需安装系统依赖。按键仍由 Rust 处理，原有用户扩展仅增加库加载入口。安装器更新 prefix/lib/quicklook/libquicklook_caja.so 与 ~/.local/share/caja-python/extensions/quicklook.py；没有修改系统命令、终端配置或默认文件夹关联。

```bash
source "$HOME/.cargo/env"
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --workspace
python3 scripts/test-space-keys.py --hosts caja --caja-entry extension --prefix "$HOME/.niri-stack/prefix"
python3 scripts/test-space-keys.py --hosts caja --caja-entry desktop --prefix "$HOME/.niri-stack/prefix"
python3 scripts/test-space-keys.py --hosts caja --caja-entry preload --prefix "$HOME/.niri-stack/prefix"
python3 scripts/test-caja-adapter.py --profile release --loader extension
python3 scripts/test-caja-adapter.py --profile release --loader preload
env -u VIPSHOME -u GDK_BACKEND python3 scripts/install-local.py --with-file-managers --with-niri
python3 scripts/build-deb.py --no-build --version 0.0.1-2
python3 scripts/test-deb.py dist/quicklook_V0.0.1-2_amd64.deb
```

首轮启动方式验证使用临时源码安装前缀 .local/caja-native-install/prefix，测试只写隔离的 HOME/XDG。本机更新记录为 .local/dev-environment/local-install-20261026T094901+0800.json 及之后记录；用户已确认直接运行 caja 恢复，未自动重启用户文件管理器。
