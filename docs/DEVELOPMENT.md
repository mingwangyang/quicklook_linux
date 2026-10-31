# 开发环境搭建

> 第一次接手先读 [交接说明](HANDOVER.md)：架构、进程模型、集成矩阵、已知陷阱、待办都在那里。
> 本文所有命令都假设你已 `cd` 到本仓库克隆出的目录（下文用 `$REPO` 指代），没有固定路径要求。

目标环境：Linux Mint 22.3 / Ubuntu 24.04（Noble），x86_64，niri + Wayland；X11 会话可用（0.0.9 起预览窗口置顶已适配）。
其他发行版可以构建，思路见[第 7 节](#7-其他发行版与自行调整)，但不在项目验证范围内。
实际执行经过和当前状态见 [安装记录](SETUP_LOG.md)。

2026-10-25 参考机环境已搭建完成：全量依赖检查缺失项为 0，基础与完整 Rust 验证程序均已编译运行通过。以下步骤保留用于重装和复查。

## 0. 三分钟概览

```bash
cd $REPO
bash scripts/install-system-deps.sh --install   # 1. 系统库（sudo 只用于 apt）
bash scripts/install-rust.sh                    # 2. Rust 1.99.1（用户级）
source "$HOME/.cargo/env"
bash scripts/check-environment.sh               # 3. 体检：缺啥会列出来
cargo build --locked --release --workspace      # 4. 构建
python3 scripts/install-local.py --with-niri --with-file-managers   # 5. 安装（可选）
```

## 1. 安装系统依赖（需要你在终端输入管理员密码）

在仓库目录运行：

```bash
bash scripts/install-system-deps.sh --install
```

脚本读取 [完整包清单](../scripts/dev-packages.txt)，执行 `sudo apt-get update` 与 `sudo apt-get --no-remove install --no-install-recommends ...`。apt 显示实际下载量、占用空间和安装计划，确认后安装。只给 apt 使用 sudo，请勿以 `sudo bash` 运行脚本。

安装前仅查看计划：

```bash
bash scripts/install-system-deps.sh --dry-run
```

2026-10-25 参考机预检：121 个新增包、0 个升级、0 个移除；软件源刷新后数量可能变化。显式选择 `libjack-jackd2-dev`，防止 apt 默认选择 JACK1 导致移除现有 Wine。`--no-remove` 会在需要移除包时中止。

依赖包括：

- 基础工具：C/C++ 编译器、pkg-config、CMake、Meson、Ninja、Git、curl。
- Wayland/GTK：GTK4、GLib、epoxy/EGL、Wayland、xkbcommon、librsvg。
- P0：GtkSourceView 5、Poppler-GLib、libmpv、libarchive。
- P1：WebKitGTK 6.0、libheif 及 HEVC/AV1 插件、LibRaw、JPEG XL、TIFF、libvips、字体及色彩库。
- 媒体/诊断/集成：FFmpeg、Ghostscript、VA-API/Mesa、vainfo、fzf、wl-clipboard、wayland-info、会话 DBus。

当前普通 Wayland 浮窗方案无需 gtk4-layer-shell；Nemo 适配通过运行时加载已安装的 GTK3/Nemo 库构建，不向 GTK4 进程混入 GTK3。libvips 纳入本次环境，用于验证超大图和扩展格式路径。

### 哪些库是编译期硬依赖，哪些只是运行时

想精简环境、或换成别的发行版时，按这个表取舍：

| 层级 | 内容 | 缺了会怎样 |
|---|---|---|
| **编译硬依赖**（`crates/daemon/build.rs` 逐个 pkg-config 探测，缺一即失败） | gtk4 / glib / gio、epoxy + EGL、libmpv、libarchive、libvips、LibRaw、fontconfig、x11、Wayland、xkbcommon | 链接失败，无法编译。这是最小可编译集 |
| **Rust 绑定硬依赖**（`crates/daemon/Cargo.toml`，同样走 pkg-config） | GtkSourceView 5、WebKitGTK 6.0、Poppler-GLib | 同上，文本/PDF/HTML 预览的绑定层 |
| **编译可选**（`tools/environment-check`，只在跑环境验证时需要） | sourceview5 / webkit6 的 `--features full` | 只影响探针的完整模式，不影响产品构建 |
| **运行时按格式加载** | libheif（HEIC）、libjxl、libtiff、媒体编解码族、ffmpeg、Ghostscript | 编译不需要；对应格式预览时才用到。deb 打包时由 shlibdeps 自动生成运行时依赖 |
| **运行时推荐/建议** | zenity（部分提示框）、python3-caja（Caja 空格扩展）、libheif 的 HEVC/AV1 解码插件 | deb 的 Recommends/Suggests；没有也能跑 |
| **开发工具链** | cmake / meson / ninja（部分 -sys crate 从源码构建时用）、rustfmt / clippy（质量门） | 只影响构建体验与 CI 检查 |

## 2. 安装 Rust（普通用户，无需管理员权限）

```bash
bash scripts/install-rust.sh
source "$HOME/.cargo/env"
```

工程固定 **Rust 1.99.1**，配置见 [rust-toolchain.toml](../rust-toolchain.toml)。这是 2026-10-25 从官方 stable manifest 核实的版本，包含 rustfmt、clippy，采用 minimal profile。

安装脚本先下载官方 rustup 引导脚本、验证已记录的 SHA-256，再执行安装。首次安装在默认位置创建 `~/.cargo` 和 `~/.rustup`；不修改 shell 配置，当前终端通过上面的 `source` 启用。已有 rustup 时复用，只补工程所需工具链。

上游引导脚本更新后可能校验失败。此时先核对新官方脚本，再更新脚本中的校验值，不能删掉校验来绕过。网络失败时保留日志并重试；本次使用官方源，未配置第三方镜像。

若希望新终端自动使用 Rust，可在自己的 `~/.zshrc` 或 `~/.bashrc` 加一行（脚本不会自动加入）：

```bash
source "$HOME/.cargo/env"
```

[官方 Rust 安装说明](https://rust-lang.org/tools/install/)、[rustup 工具链固定方式](https://rust-lang.github.io/rustup/overrides.html)。

## 3. 检查环境

```bash
bash scripts/check-environment.sh
bash scripts/check-environment.sh --gpu
```

第一条检查工程固定 Rust、工具、所有原生开发库和 apt 包；有缺项时返回非零。第二条另外连接当前桌面会话，检查 Wayland 协议与 VA-API 驱动。GPU 检查需要在已登录的 Wayland 会话中执行。

安装和检查日志自动保存到仓库的 `.local/dev-environment/`；该目录已忽略，不提交下载文件、构建缓存及本机日志。

实际编译、链接和会话验证：

```bash
bash scripts/verify-rust-gtk.sh --basic
bash scripts/verify-rust-gtk.sh --full
```

基础验证编译 Rust + GTK4，连接原生 Wayland 和会话 DBus，不打开窗口。完整验证需要先安装系统依赖，另外编译 GtkSourceView/WebKit 绑定，并实际调用 Poppler、libmpv、libarchive、HEIF、RAW、JXL、libvips 的版本接口检查链接。

验证程序在 `tools/environment-check/`，固定 gtk4 0.11.5、sourceview5 0.11.2、webkit6 0.6.1，已核对三者使用同一代 gtk4/glib/gio。Cargo.lock 固定传递依赖；构建输出统一写入根目录 `target/environment-check/`。首次编译需要下载 crates。

当前参考机系统 GTK4 为 4.14.5，Rust 绑定只启用到 `v4_14`。功能、性能和 libmpv 实际播放另行验收；编译验证通过不等于 P0/P1 功能已实现。

## 4. 可复现范围与回退

Rust 版本固定；系统包来自配置好的 Mint/Ubuntu 仓库，实际版本记录在日志中，不为复现开发环境冻结系统安全更新。后续 Rust 应用与环境验证程序各自提交 Cargo.lock。

系统依赖是共享软件包，记录新增包后按需人工处理，不自动卸载；Rust 如需彻底移除可运行官方 `rustup self uninstall`，但这会影响该用户其他 Rust 工程。系统/Rust 环境脚本不修改桌面配置。产品安装器及其配置变更见下一节。

## 5. 构建和安装产品

普通用户执行：

```bash
cd $REPO
source "$HOME/.cargo/env"
cargo build --locked --release --workspace
python3 scripts/install-local.py --with-niri --with-file-managers
```

默认前缀 `~/.niri-stack/prefix`，可用环境变量 `QUICKLOOK_PREFIX` 改成任意绝对路径（例如装到 `~/.local`）：

```bash
QUICKLOOK_PREFIX="$HOME/.local" python3 scripts/install-local.py --with-niri
```

安装五个原生可执行程序（CLI、broker、viewer、Nautilus 桥接、剪贴板帮助程序）、Nemo 适配库/启动器、独立文件选择入口、桌面入口、用户 DBus 服务及文件管理器入口。开发调试可加 `--debug`，须先构建 debug workspace。安装器参数：

| 参数 | 作用 |
|---|---|
| `--with-niri` | 在 niri 配置中加入 quicklook.kdl include、浮窗规则与 Ctrl+Alt+P 选文件快捷键（写入前先 `niri validate`） |
| `--with-file-managers` | 安装 Nautilus 原生预览入口与 Yazi 配置；Dolphin/Nautilus 脚本/Yazi 插件默认提供 |
| `--no-autostart` | 不写登录自启项 |
| `--no-start` | 不立即（重）启动常驻服务 |
| `--dry-run` | 只打印将要改动的文件 |

--with-niri 在现有配置中加入的改动和备份路径都记录在 `.local/dev-environment/local-install-*.json`。

安装器不会自动重启已经运行的 Nemo。要启用空格联动，先在"文件"应用菜单退出，结束其中的复制任务，再从"文件（Quick Look）"启动。二进制指纹不同的 Nemo 会跳过适配，仍可使用 CLI 和右键入口。无需安装 Wine。

更新后执行 quicklook --quit，下次打开加载新程序。卸载前退出服务，运行 `python3 scripts/uninstall-local.py`（支持 --dry-run），恢复 niri 配置备份或移除新增 include/quicklook.kdl。保留系统共享依赖，不自动卸载 apt 包。

不想装进用户目录、想用系统包管理器的话，走 deb 路线，见 [PACKAGING.md](PACKAGING.md)（`scripts/build-deb.py` 产物用 `sudo apt install ./dist/….deb` 安装）。

## 6. 自查

```bash
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/smoke-test.py --profile release
python3 scripts/extra-fixtures.py --download-samples
python3 scripts/smoke-test.py --profile release
python3 scripts/test-nemo-adapter.py --profile release
python3 scripts/robustness-test.py
python3 scripts/benchmark.py --profile release
```

基础烟测生成本地样本；扩展脚本增加原生编码、108MP 图片、长动画，可选下载上游 HEIC/RAR/相机 RAW 测试文件。样本、截图、日志位于忽略的 `.local/`。Nemo 测试隔离 DBus、禁用测试会话的无障碍桥接，避免占用当前会话无障碍 socket。不要将整个私有测试会话接到现有无障碍总线上。

## 7. 其他发行版与自行调整

项目验证范围是 Mint 22.x / Ubuntu 24.04，但结构上不绑定发行版。自行调整时按下面的对应关系换包名即可：

### 7.1 原生库 → pkg-config 名称 → 常见发行版包名

编译期探测全部走 pkg-config，`scripts/check-environment.sh` 也用同一组名称体检。换发行版时保证这些 `.pc` 文件存在即可：

| pkg-config 模块 | 用途 | Debian/Ubuntu 包 | Fedora 包 | Arch 包 |
|---|---|---|---|---|
| `gtk4` | UI 框架 | libgtk-4-dev | gtk4-devel | gtk4 |
| `glib-2.0` / `gio-2.0` | 基础库 | libglib2.0-dev | glib2-devel | glib2 |
| `epoxy` + `egl` | GL 加载 | libepoxy-dev + libegl1-mesa-dev | libepoxy-devel + mesa-libEGL-devel | libepoxy + mesa |
| `wayland-client` | Wayland | libwayland-dev | wayland-devel | wayland |
| `xkbcommon` | 键盘 | libxkbcommon-dev | libxkbcommon-devel | libxkbcommon |
| `librsvg-2.0` | SVG | librsvg2-dev | librsvg2-devel | librsvg |
| `gtksourceview-5` | 文本高亮 | libgtksourceview-5-dev | gtksourceview5-devel | gtksourceview5 |
| `poppler-glib` | PDF | libpoppler-glib-dev | poppler-glib-devel | poppler-glib |
| `mpv` | 媒体 | libmpv-dev | mpv-devel | mpv |
| `libarchive` | 压缩包 | libarchive-dev | libarchive-devel | libarchive |
| `webkitgtk-6.0` | HTML/Markdown | libwebkitgtk-6.0-dev | webkit2gtk-6.0-devel（Fedora 41+） | webkitgtk-6.0 |
| `vips` | 大图/扩展格式 | libvips-dev | vips-devel | libvips |
| `libraw` | 相机 RAW | libraw-dev | LibRaw-devel | libraw |
| `fontconfig` | 字体 | libfontconfig-dev | fontconfig-devel | fontconfig |
| `libheif` / `libjxl` / `libtiff-4` | HEIC / JXL / TIFF（运行时） | libheif-dev / libjxl-dev / libtiff-dev | 相应 -devel | 相应包 |
| `libva` | VA-API（运行时诊断） | libva-dev | libva-devel | libva |

> 表中 Fedora/Arch 包名以常见命名为准，随版本可能变化；以 `pkg-config --exists <模块>` 实测为准。装完跑一遍 `bash scripts/check-environment.sh`，缺什么它会直接列出来。

### 7.2 调整 Rust 工具链

- 工具链版本写在 [rust-toolchain.toml](../rust-toolchain.toml)（当前 1.99.1，minimal profile + rustfmt + clippy）。rustup 会自动跟随该文件，进仓库目录即用指定版本。
- 想用更新的 stable：改该文件的 `channel`，然后 `cargo build --locked`；若 cargo 因 edition/依赖报版本不足，同步看 `Cargo.toml` 的 `rust-version`。
- 离线/内网环境：在有网机器上 `rustup toolchain install 1.99.1` 后拷贝 `~/.rustup/toolchains/`，或使用发行版打包的 rust（版本 ≥ `rust-version` 即可，CI 之外不强制 1.99.1）。
- crates 镜像加速：`~/.cargo/config.toml` 里配置 `[source.crates-io] replace-with = '<镜像>'`（例如字节/中科大镜像），本项目无特殊 source 配置，用户级镜像直接生效。

### 7.3 增删系统依赖

- 增删包改 [scripts/dev-packages.txt](../scripts/dev-packages.txt)（注释行/空行会被忽略，包名做格式校验），`install-system-deps.sh` 与 `check-environment.sh` 都读这一份。
- 前面那张"编译硬依赖 vs 运行时"表（第 1 节）是取舍依据：只想编译通过，硬依赖那一行即可；想要完整格式覆盖，按 P1/运行时行补齐。
- CI 里的安装清单是从 dev-packages.txt 推导的（去掉纯运行期工具），改动清单后如 CI 缺包，同步看 [.github/workflows/ci.yml](../.github/workflows/ci.yml) 的排除列表。

### 7.4 NixOS / 容器

- NixOS：项目未带 shell.nix/flake，可用 `nix-shell -p` 按第 1 节硬依赖列表临时给环境；pkg-config 名称一致即可编译。
- Docker：基础镜像选 ubuntu:24.04，`apt-get install` dev-packages.txt 内容 + git + python3，容器内跑 `cargo build` 不需要 Wayland 会话；运行/烟测才需要（可用 Xephyr，见 HANDOVER）。

### 7.5 常见问题

- **`mpv: pkg-config` 报错**：装的是 `mpv` 播放器而不是 `libmpv-dev` 开发包；编译要 -dev 包。
- **WebKitGTK 装了但探测失败**：确认是 6.0 API 版（`pkg-config --modversion webkitgtk-6.0`），webkit2gtk-4.1 不通用。
- **JACK 冲突**：Ubuntu 上 `libjack-jackd2-dev` 与 JACK1 二选一，脚本显式选 JACK2 防止连带移除 Wine；不关心 Wine 可不管。
- **首次构建很慢**：正常。`--locked` 下首次要下载 crates 并编译约 400 个依赖；之后增量构建只动改动 crate。

## 8. 文件管理器可选集成

--with-file-managers 安装 Nautilus 原生预览激活入口及 Yazi 的初始化/按键配置。Dolphin 右键动作、Nautilus 脚本、Yazi 插件文件默认提供；不加该选项时不修改 Yazi 按键及 Nautilus 默认预览服务。Nemo 适配沿用二进制指纹保护。

既有配置会备份；已有同名快捷键不覆盖。Yazi 如果使用无法直接追加的行内按键数组，则原文件保持不变，另写 quicklook-keymap.toml 供合并，安装器会提示。文件管理器集成统一由 `quicklook --apply-hosts` 落地（设置窗口保存时也会调用），预览键取自配置，默认空格。

各宿主验证状态与版本限制（含 Dolphin 目前无法支持空格预览）见 [FILE_MANAGERS.md](FILE_MANAGERS.md)。
