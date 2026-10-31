# 交接说明

> 写于 2026-10-25。面向接手这个工程的人：读完这一篇应该能独立构建、验证、排查和继续开发。
> 更细的历史结论见 `docs/IMPLEMENTATION.md`（为什么这么做）与 `docs/VALIDATION.md`（真机测过什么）。

## 一、这是什么

Linux 上复刻 macOS Quick Look 的原生预览器：在文件管理器里选中文件按空格，立刻弹出预览窗口，再按空格关闭。Rust + GTK4，不依赖 Electron / Web 技术栈（除了 Markdown、HTML、Office 这类富文本用 WebKit 渲染）。

- 仓库：`git@github.com:mingwangyang/quicklook_linux.git`
- 分支：`main`（稳定版，对外发布线）+ `dev`（日常开发线）
- 分支模型：开发在 `dev` 上进行（功能分支可从 `dev` 拉出），阶段性稳定后合回 `main` 并打版本标签（如 `V0.0.9`）；对外只需要看 `main`
- 目标环境：Linux Mint 22.3 + niri 26.04（Wayland）+ noctalia 面板
- 许可证：GPL-3.0-or-later
- 当前阶段：已按最新要求检查安装边界、修复 Caja，提供 Ubuntu 24.04 / Mint 22.x amd64 deb。见 [本轮检查](REVIEW.md) 与 [安装/迁移](PACKAGING.md)。

## 二、当前状态

**可用。** 六种文件管理器的入口都已接通（其中三种非侵入、三种靠进程注入），预览格式覆盖到 Office 与嵌套压缩包。

```
状态：功能完整度 P0/P1 全部完成，P2 完成 Office、嵌套包、托盘、Thunar/Nemo/Caja 注入
质量门：133 项测试全绿，clippy 0 警告，fmt 干净
构建：cargo build --release --locked --workspace 通过
安装：已装到 ~/.niri-stack/prefix，日常可用；安装/卸载均为事务化脚本
```

代码规模 14 121 行 Rust：

| crate | 行数 | 职责 |
|---|---|---|
| `crates/core` | 2565+ | 文件分类、配置、预览键（含共享的按键状态机 `keys::classify`）、宿主集成（`hosts.rs`）、niri 窗口规则（`niri.rs`）、D-Bus 协议常量 |
| `crates/daemon` | 7356 | GTK4 预览窗口、各种格式的渲染后端、设置窗口 |
| `crates/cli` | 1467 | `quicklook` 命令行 + `quicklookd` 常驻代理（含托盘） |
| `crates/thunar-adapter` | 751 | Thunar 的 LD_PRELOAD 适配器 |
| `crates/caja-adapter` | 550 | Caja 的 LD_PRELOAD 适配器（符号从 `RTLD_DEFAULT` 查） |
| `crates/nemo-adapter` | 699 | Nemo 的 LD_PRELOAD 适配器 |
| `crates/clipboard` | 114 | 独立进程持有 Wayland 剪贴板 |

## 三、进程模型

```
文件管理器 / 终端 / 托盘
        │  D-Bus  org.quicklook.Preview
        ▼
   quicklookd（常驻，GIO-only，无 GTK）
        │  D-Bus  org.quicklook.Viewer
        ▼
   quicklook-viewer（GTK4 窗口，空闲 viewer_grace_secs 秒后退出）
```

- `quicklookd` 是唯一常驻进程，负责代理请求、按需拉起 viewer、托盘图标（StatusNotifierItem）。
- 托盘注册成功后 `quicklookd` **不再按 `idle_timeout_secs` 退出**（否则右键菜单会随代理消失）；托盘不注册时才回到空闲退出。登录自启由配置项 `autostart`（默认关闭）控制，条目由 `hosts.rs` 写 `~/.config/autostart/org.quicklook.Preview.desktop`；源码安装器的 `--no-autostart` 只是把该配置项置为 false。
- `quicklook-viewer` 按需启动、空闲自杀，所以「快」的关键在冷启动路径（见性能一节）。
- `quicklook-d` 与 viewer 都用 **`NON_UNIQUE`** 应用标志，避免 GApplication 单实例语义干扰。
- 分离窗口（`Ctrl+D`）是另起一个 `quicklook-viewer --detached` 进程，用不同 app-id（`org.quicklook.Preview.Detached`）以便 niri 用不同窗口规则处理（弹窗浮动、分离窗口平铺）。

## 四、构建与验证

```bash
source "$HOME/.cargo/env"          # 工具链在 ~/.cargo / ~/.rustup，Rust 1.99.1

# 构建
cargo build --release --locked --workspace

# 质量门（提交前跑这三条）
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets
cargo test --locked --workspace          # 133 项

# 冒烟（会生成 50+ 夹具，含 Office、嵌套包、108MP 图、PDF）
env -u VIPSHOME python3 scripts/smoke-test.py --profile release

# 安装到 prefix（含文件管理器集成 + niri 配置）
env -u VIPSHOME python3 scripts/install-local.py --with-file-managers --with-niri

# 卸载（--dry-run 只打印；--purge 连配置和缓存一起删）
env -u VIPSHOME python3 scripts/uninstall-local.py --dry-run
```

> **重要陷阱**：在这台机器的自动化 shell 里有两个变量会污染测试：
> - `VIPSHOME=/target` 会让 libvips 去 `/target/...` 找模块，heif/magick/jxl **全部静默失效**（表现为 BMP/AVIF 打不开）。
> - `GDK_BACKEND=x11` 会让 viewer 伪装成 X11，测不到原生 Wayland 路径。
>
> 所以自动化命令统一用 `env -u VIPSHOME -u GDK_BACKEND`。用户真实桌面会话里没有这两个变量；验证 `--status` 的 `backend` 应为 `GdkWaylandDisplay`。

装出来的东西：

```
~/.niri-stack/prefix/bin/   quicklook quicklookd quicklook-viewer quicklook-settings
                            quicklook-clipboard quicklook-nautilus-bridge
                            quicklook-pick quicklook-nemo quicklook-thunar
~/.niri-stack/prefix/lib/quicklook/  libquicklook_nemo.so libquicklook_thunar.so
```

## 五、已实现的功能

### 预览格式

| 类别 | 实现方式 |
|---|---|
| PNG/JPEG/GIF/WebP/BMP/ICO/TIFF/HEIC/AVIF/JXL/SVG | libvips；**vips 失败时退回 `image` crate 内置解码**（见已知问题 3） |
| 视频 / 音频 | libmpv（Render API + GtkGLArea），支持暂停、逐帧、变速 |
| GIF / WebP 动图 | 自绘逐帧播放（≤240 帧 / 96 MiB），长图走 libmpv 或双帧队列 |
| PDF / AI | Poppler；连续滚动、缩略图侧栏、页码跳转 |
| 文本 / 代码 | GtkSourceView，支持 UTF-8 / GBK / GB18030 / Windows-1252 切换 |
| Markdown / HTML / RTF | WebKitGTK（JS 默认关闭 + CSP）；含 ```mermaid 的 Markdown 用内置 Mermaid 渲染 |
| JSON / XML / YAML | 懒加载树；JSON 默认源码视图 |
| CSV / TSV | 分隔符嗅探 + RFC 4180，渲染成表格（粘性表头/行号/数值右对齐），可切源码 |
| Office：docx/xlsx/pptx | **自己解析 OOXML**（roxmltree + libarchive），只保证文字/层级/列表/表格/工作表/放映顺序，不做版式还原 |
| 压缩包 | libarchive；ZIP/TAR/7z/RAR，拒绝路径穿越与链接 |
| 嵌套压缩包 | 包内文件可再展开，最多 4 层，`Backspace`/`Esc` 返回 |
| 目录 | 列表 + 统计 |
| 字体 | ttf-parser 渲染样张 |
| EPS / RAW / PSD | Ghostscript / LibRaw / 自写合成图读取 |

### 交互

`Space` 关闭 · `↑↓` 浏览 · `Enter` 打开 · `Esc` 返回/关闭（全屏时先退出全屏）· `F` 全屏 · `P` 固定 · `I` 属性 · `Ctrl+D` 分离窗口 · `Ctrl+C` 复制文件或选中文本 · `T` PDF 侧栏 · `R` 旋转 · `0/1` 缩放 · `W` 换行 · `E` 编码 · `K` 暂停。

2026-10-26 已移除 400 ms 切换冷却；现在按事件状态去重，两次完整空格之间没有最小间隔，见 FILE_MANAGERS.md 的本轮验证。预览窗口默认浮动，可在设置里改成默认平铺（`window_mode` → niri 规则）。

命令行能力见 `quicklook --help`：`--show/--pick/--next/--prev/--close/--status/--quit/--pin/--unpin/--back/--page/--extract/--screenshot/--settings/--apply-hosts/--stdin0/--from-nautilus`。

## 六、文件管理器集成矩阵（重点，有侵入性说明）

以下旧路径中的 prefix/bin/quicklook-{nemo,thunar,caja} 已迁移到 XDG_DATA_HOME/quicklook/launchers；库仍留在安装前缀。当前路径与操作以 [FILE_MANAGERS.md](FILE_MANAGERS.md) 为准，不再创建 PATH 同名命令。

| 宿主 | 空格接管方式 | 落盘文件 | 侵入性 |
|---|---|---|---|
| **Dolphin** | 右键菜单项；**目前无法支持空格预览**（空格是宿主自己的打开/重命名行为：23.08 无快捷键接口，26.04 + KIO 6.24 起才能在宿主手动绑空格）；写/删菜单后自动重建 KSycoca | `~/.local/share/kio/servicemenus/quicklook.desktop` | 无 |
| **Nautilus** | 抢占原生预览器（`org.gnome.NautilusPreviewer2`） | `~/.local/share/nautilus/scripts/Quick Look`、`~/.local/share/dbus-1/services/org.gnome.NautilusPreviewer.service` | 无 |
| **Nemo** | 默认空格：抢占原生预览总线（`org.nemo.Preview`），Nemo 自己把空格绑定到 `ShowFile`；改键：LD_PRELOAD 启动器 + `gtk_key_snooper_install` 在视图处理前接管，并吞掉裸空格。两种模式都先注册对象再抢名，避免首次激活竞态 | 默认：`~/.local/share/dbus-1/services/org.nemo.Preview.service`；改键：`prefix/bin/quicklook-nemo`、`~/.local/share/applications/nemo.desktop`；两者都含 `~/.local/share/nemo/actions/quicklook.nemo_action` | 默认无；改键有 |
| **Yazi** | 配置（插件 + keymap + init.lua 托管块） | `~/.config/yazi/plugins/quicklook.yazi/`、`~/.config/yazi/keymap.toml`、`~/.config/yazi/init.lua` | 无 |
| **Thunar** | **LD_PRELOAD 注入** + desktop 覆盖 | `prefix/bin/quicklook-thunar`、`~/.local/share/applications/thunar.desktop`、`~/.config/Thunar/uca.xml` | **有** |
| **Caja** | python3-caja 用户扩展加载 **Rust 按键适配**；保留显式 LD_PRELOAD 启动器与五个 desktop 入口 | `$XDG_DATA_HOME/quicklook/launchers/quicklook-caja`、`~/.local/share/applications/caja*.desktop`、`~/.local/share/caja-python/extensions/quicklook.py` | **有** |

为什么 Thunar / Caja 必须注入：它们没有「预览器」D-Bus 接口，没有热键插件 API，也不像 Yazi 那样读配置。要拦下空格并读到选中项，只能在进程内挂钩。注入点用 `g_application_run`（跨 DSO 调用走 PLT 可拦截）+ `gtk_key_snooper_install`（GTK 公开 API），**不打补丁、不改内存、不动宿主的二进制**。Caja 1.26 还多一层：它连 Nemo 那种 `.caja_action` 动作文件都没有，右键只能走发行版自带的 python3-caja 扩展。

Nemo 默认空格为什么不用注入：Nemo 6 的列表/图标视图自己处理 `GDK_KEY_space`，然后调用 `org.nemo.Preview.ShowFile`（参数是 URI、X11 window id、`close_if_already_visible=true`）。列表视图会先把按键消费掉，窗口级的 LD_PRELOAD 钩子（`key-press-event` 信号）根本收不到事件；而新启动的适配器进程只会被已有实例转交后退出。所以默认空格正确做法是提供同名 D-Bus 服务，让 broker 直接实现 `ShowFile`/`Close`——这与 Nemo 是否已经在运行、从哪里启动都无关。

Nemo 改键为什么又能注入：GTK 的 **key snooper**（`gtk_key_snooper_install`）在事件分发给焦点控件之前运行，因此能在 Nemo 视图消费 Space 之前看到按键。`--apply-hosts` 在 `preview_key != Space` 时生成 `quicklook-nemo` 启动器（`LD_PRELOAD`）并覆盖 `nemo.desktop`，适配器安装 snooper：按下配置键时读当前视图选择并发 `Toggle`，裸空格直接返回 `TRUE` 吞掉，宿主自己的空格预览就不会再弹。改回空格后启动器与 desktop 覆盖会被删除，恢复原生 service。旧版的 `nemo.desktop` 覆盖与 `quicklook-nemo` 启动器在切换时也会清理（只删内容里带 `quicklook-nemo` 标记的文件）。

代价与风险：

1. 直接运行 `thunar` / `/usr/bin/nemo` 的适配限制仍在；Caja 已安装 python3-caja 并应用接管时，普通 `caja` 命令会经用户扩展加载 Rust 适配；
2. Thunar / Caja / Nemo 已在运行但与当前库不匹配时（没加载、加载的是旧版、或重装后只剩 `(deleted)` 映射），启动器会比较 `/proc/<pid>/maps` 的 inode，弹窗询问是否重启（`thunar --quit` / `caja --quit` / `nemo --quit` 后重新拉起），取消则发桌面通知说明原因；Caja 的 `--quit` 是 `-q`（`caja --help-all` 可见），不要用 `pkill`——退不干净时宁可停下提示用户，也不要留两个实例抢焦点；
3. 系统升级改了 `thunar.desktop` / `nemo.desktop` / `caja*.desktop` 后，我们的覆盖副本会漂移（我们只替换 `Exec=` 行，其余原样复制）；
4. 宿主大版本升级可能改内部结构。Nemo 默认空格走公开 D-Bus 协议，风险最低；Nemo 改键、Thunar 与 Caja 都依赖进程内 snooper 和类名/列类型定位，升级后仍需复测。

集成文件的写入全部在**用户 XDG 目录**，不需要 root，不碰 `/usr`。覆盖系统 desktop 用的是 XDG 优先级（`~/.local/share/applications` 优先于 `/usr/share/applications`）。`quicklook --apply-hosts` 幂等：内容相同不写；`--apply-hosts --dry-run` 只报告将写/将删的文件；`--remove-hosts` 按配置反向清理（共享文件只剥离 `BEGIN/END quicklook managed integration` 托管块，用户自己的条目原样保留）。

## 七、用户配置

配置在 `~/.config/quicklook/config.toml`，**默认不存在**（全用内置默认值），设置窗口保存时才创建。

| 项 | 默认 | 说明 |
|---|---|---|
| `preview_key` | `"Space"` | 全局默认预览键，支持 `Ctrl/Shift/Alt` + `Space/Return/F1-F12`（**刻意不支持字母键**，字母 keyval 随 Shift 层变化会让适配器比对产生歧义） |
| `preview_key_nemo` / `_thunar` / `_caja` / `_yazi` / `_nautilus` | `""` | 单个宿主的覆盖键；空字符串表示跟随 `preview_key`，写坏的值也退回跟随（不会替用户猜一个键）。Dolphin 新版在宿主中设置快捷键、旧版没有该接口，所以本工具没有覆盖项 |
| `theme` | `"light"` | 预览界面主题：`light` 或 `dark`；未知值退回浅色。设置窗口、GTK 部件、文档 HTML 与源码配色一起切换 |

| `mermaid` | `true` | 是否渲染 Markdown 里的 ```mermaid 流程图；关闭后按普通代码块显示。只对含图的 Markdown 打开 JavaScript，其余视图照旧关闭 || `window_mode` | `"floating"` | 预览窗口默认布局：`floating` 或 `tiled`；未知值退回浮动。Wayland 客户端不能自己要求浮动，实际生效的是 `~/.config/niri/quicklook.kdl` 里的 `open-floating`，由 `core::niri` 在设置保存和 broker 启动时重写并让 niri 重新加载。独立窗口（`org.quicklook.Preview.Detached`）始终平铺，选择器与设置窗口始终浮动 |
| `hijack_native` | `false` | 是否接管宿主原生空格预览（关掉后注入与 Nautilus 预览器都不装）；`preview_key` 非空格时还会决定 Nemo 走启动器接管还是保持原生 |
| `autostart` | `false` | 登录时自动启动常驻 broker（XDG autostart 条目） |
| `host_nemo/dolphin/nautilus/thunar/caja/yazi` | `true` | 每个宿主的独立开关 |
| `cache_mb` | 200 | 预览缓存上限 |
| `image_max_edge` | 2560 | 图片最大边 |
| `viewer_grace_secs` | 3 | 窗口关闭后多久退出 viewer |
| `idle_timeout_secs` | 600 | 代理空闲退出 |
| 其他 | — | `text_chunk_bytes`、`max_text_buffer_bytes`、`max_archive_entries`、`max_extracted_bytes`；`nemo_compat` 在 `host_nemo && hijack_native` 时自动开启，一般不用手写 |

设置窗口：`quicklook --settings`（或托盘右键 →「设置…」）。五个区段：预览键（全局默认 + Nemo / Thunar / Caja / Yazi / Nautilus 各自「跟随全局默认 / 指定按键」）、窗口（默认布局浮动/平铺）、外观（浅色/深色）、文件管理器集成、渲染与缓存。「保存设置」只写选项；文件管理器接入通过独立的「应用文件管理器接入 / 注入」按钮应用，也可用「撤销全部接入」恢复宿主入口；已运行的宿主不会热加载，需重启对应的文件管理器（Nemo 请从应用菜单重启，才会走到启动器）。主题对设置窗口立即生效，对之后新开的预览窗口生效；默认布局会重写 niri 规则并触发 `niri msg action load-config-file`，下一次打开预览即生效（未安装 niri 集成时给出提示，不报错）。

托盘图标（StatusNotifierItem，由 `quicklookd` 提供）：左键 = 选文件预览，右键 = 设置… / 重启 / 退出。

“重启”的动作是：旧进程先 `spawn quicklookd --replace`，再走正常的 `Quit`；新进程注册好对象后轮询 `RequestName`，等旧连接释放 `org.quicklook.Preview` 与 `org.nemo.Preview` 再接管。这样不需要手动 kill，也不会出现新进程因名字被占而直接退出。重启会关闭已显示的预览窗口，文件管理器窗口不受影响。

## 八、性能（release，历史基准）

| 场景 | 数值 |
|---|---|
| 冷启动到内容就绪 | 340.88 ms（其中 viewer 数据就绪 63.32 ms） |
| 热重开（中位 / 最大） | 18.81 / 31.25 ms |
| 缓存命中切文件（中位 / 最大） | 17.71 / 50.00 ms |
| 10000 行文本 | 34.28 ms |
| 空闲常驻内存 | 6664 KiB |

口径说明：这些是「请求往返到 viewer 报告内容就绪」的时间，**不含合成器实际出帧**。需求文档里的 <80ms 目标是按这个口径算的，用户可感知的延迟没有独立测量过。

## 九、已知问题与陷阱

1. **`is_fullscreen()` 不知道合成器全屏**。niri 的 `Mod+F` 全屏后，`gtk_window_is_fullscreen()` 仍返回 `false`。因此 `Controller` 自己记了一份 `fullscreen: Cell<bool>`，toggle 时取 `自己记的 || is_fullscreen()` 作为判据。改这块时别退回只查 GTK。
2. **dbusmenu 节点必须是三元组** `(id, a{sv}, av)`。少写第三段时 `gdbus` 打印出来「看着正常」（会省略空的尾段），但宿主按签名反序列化会把整份菜单判为无效，右键菜单就出不来。真正的报错在 stderr：`Type of return value is incorrect: expected '(u(ia{sv}av))'`。另外 `av` 的元素必须真的包一层 `v`（`array_from_iter_with_type(VariantTy::VARIANT, …)`），否则运行时 panic。
3. **libvips 8.15 起不再自带 BMP 读取器**（靠 `vips-magick` 模块）。模块缺失时 P0 级的 BMP/ICO 会打不开，所以 `backend.rs` 的 `Kind::Image` 分支做了「vips 失败 → `image` crate 兜底」，并在状态栏注明走了兜底。
4. **托盘图标必须重建 icon-theme.cache**。PNG 写进 `~/.local/share/icons/hicolor` 后若不跑 `gtk-update-icon-cache`，宿主按名字查不到，会显示自己的兜底图形（曾为此排查很久）。现在 `hosts::apply_icons` 与安装器都会自动重建。图标是编译期 `include_bytes!` 的，**改 SVG 必须重新 `cargo build --release`**。
5. **Nemo/Thunar 适配器修改后不影响已运行的实例**，必须退出文件管理器再启动。重装会覆盖 `.so`，旧进程仍映射着已被替换的 inode；Thunar 与 Nemo（改键模式）的启动器都会比较 `/proc/<pid>/maps` 里的 inode 与当前文件，发现 `(deleted)` 或 inode 不一致就会提示重启（只匹配路径会把这种实例误判为已适配）。
6. **`pkill -f 'quicklook'` 会自杀**：它匹配到自己这条 `bash -c` 命令行。用 `pgrep -x` 拿 pid 再 `kill <pid>`；`pgrep -x quicklook-viewer` 因进程名超过 15 字符会失配，改用 `ps -eo pid,cmd | grep '[q]uicklook-viewer'`。误杀过一次用户的 Thunar 和一次 Nemo 窗口。
7. **Wayland 输入注入不可用/危险**：`gdk_test_simulate_key` 在 Wayland 后端不进事件管线；`ydotool key space` 实际发出的是字母 `s`，`ydotool` 注入组合键还曾把桌面弄进锁屏。验证按键交互一律请用户手动按。
8. **D-Bus 调试语法**：`gdbus call … Show '["/path"]' -a 类型参数`；`Extract` 的 `u` 参数要写裸数字 `Extract 0`；负数要加 `--` 分隔（`GetLayout 0 -- -1 "@as []"`）；返回值是 `('{…json…}',)`，要 `ast.literal_eval(raw)[0]` 再 `json.loads`。
9. **会话总线会自动激活已安装的 `quicklookd`**，所以拿 `target/debug/quicklook` 测其实测的是安装版。要测 debug 版：先停掉已安装的代理，直接起 `target/debug/quicklook-viewer`，再用 `gdbus --dest org.quicklook.Viewer` 驱动。
10. **niri 26.04 不支持 `default-window-width/height`** 这两个窗口规则键，`niri validate` 会报 unexpected node。
11. **主题不能只靠 GTK 的深色偏好**：早期网页内容写死 `Adwaita-dark`，GTK 部件又跟随系统，浅色桌面上会出现「浅色标题栏 + 深色内容区」。现在 `theme` 同时控制三处：`gtk-application-prefer-dark-theme`、`style.css` / `style-dark.css`、WebKit 的 `web.css`（`:root.dark` 变量）与 SourceView 的 `Adwaita` / `Adwaita-dark` 方案。默认 `light`，即使系统是深色也显式关掉 GTK 深色偏好。新增有色样式时优先用 `web.css` 里的 CSS 变量，不要写死深色。
12. **重复按键不能靠全局时间窗去猜**：2026-10-26 已删除 Controller::allow_toggle、TOGGLE_COOLDOWN 和 last_toggle。公共 Toggle 每次明确请求都生效，不能因为距离上次不到 400 ms 就忽略。GTK3 适配器用按下/松开状态去重，在 focus-in-event 清理失去焦点期间遗留的状态；viewer 只在收到完整 Space 按下/松开后关闭，避免焦点移交时重复按下触发开关循环。不同宿主的原生协议未必提供原始键盘事件，不要再用 URI 相同加时间阈值替代物理按键状态。
13. **PDF 缩略图侧栏每次换页都会重建**：`SingleSelection` 必须显式 `set_selected(当前页)`，否则高亮永远停在第 1 页；`ListView::scroll_to` 在列表尚未 map/分配时是空操作，要等 `connect_map` 后再延迟一帧调用，长文档才会滚到当前页（50 页 PDF 第 34 页已验证）。
14. **安装器的 host 集成回滚**：`--apply-hosts` 没有逐文件备份，所以安装失败时不能只回滚二进制。正确顺序是：若原先没有 CLI，先 `--remove-hosts`；若有旧 CLI，先回滚二进制，再用旧版本 `--apply-hosts` 恢复配套集成。`scripts/install-local.py` 的 `except` 块就是这么做的，修改时别简化掉。
15. **Nemo 的空格轮不到窗口级钩子，但轮得到 GTK key snooper**：Nemo 的列表/图标视图在 `key-press-event` 里处理 Space 并调用 `org.nemo.Preview.ShowFile`，返回 `TRUE` 后事件不再冒泡。因此 LD_PRELOAD 的**窗口级**钩子收不到空格。默认键的正确接法是由 broker 直接占用 `org.nemo.Preview`；改键后的接法是 `gtk_key_snooper_install`——它跑在事件分发给焦点控件之前，能先看到 Space。snooper 必须在 `preview_key != Space` 时把裸空格返回 `TRUE` 吞掉，否则 Nemo 原生预览会和配置键的预览同时弹。改回空格时 `--apply-hosts` 会撤掉启动器和 `nemo.desktop` 覆盖，恢复原生 service。不要再恢复「窗口级钩子直接挡空格」的旧方案。
16. **文件选择器的 app-id 是 `xdg-desktop-portal-gtk`**：GTK4 的 `FileDialog` 走 portal，niri 规则如果只匹配 `org.quicklook.Preview` 不会命中，选择器会被平铺进当前列。`integration/niri.kdl` 里有一条按标题匹配 portal 的 `open-floating true` 规则，删掉就会复发。
17. **预览窗口必须主动获得焦点**：只 `set_visible(true)` 时新窗口在 niri 下不会抢焦点，按键仍落在文件管理器里；下一次空格又被宿主当成一次切换，用户看到的就是「窗口闪一下立刻消失」。现在 `ui/mod.rs` 的 `show()` 调 `window.present()`，niri 规则对预览窗口用 `open-focused true`。注意只处理 `key-pressed`、不处理 release，才能保证「触发预览的那次空格松开」不会把窗口关掉。
18. **Dolphin 服务菜单写入后必须重建 KSycoca**。Dolphin 不实时扫 `~/.local/share/kio/servicemenus`，菜单来自 `~/.cache/ksycoca5_*` 缓存；只写 `.desktop` 不刷新时，菜单要等别的原因触发重建才出现，用户看到的就是「装了但没集成」。`hosts::apply_at_with` 现在在 Dolphin 开关启用或撤销了已有文件时自动跑 `kbuildsycoca5 --noincremental`（回落 `kbuildsycoca6`/`kbuildsycoca`），且只在真实 `XDG_DATA_HOME` 上执行；测试和 `--dry-run` 用临时目录，不会碰用户缓存。别为了「省一次重建」把条件改回「仅内容变化时」——内容没变但缓存过期正是本次故障的场景。重建后通常无需重启 Dolphin；已在运行的窗口若仍看不到菜单，重启一次最稳。缓存重建后通常无需重启 Dolphin，但已经运行的窗口若仍看不到菜单，重启一次最稳。
19. **D-Bus 激活竞态：对象必须先注册再 `RequestName`**。Nemo 的第一次空格会同时触发 `org.nemo.Preview` 激活，排队的 `ShowFile` 在 broker 拿到名字的瞬间就被派发。旧顺序是先抢名再 `register_object`，于是第一次调用稳定返回 `UnknownMethod: Object does not exist at path /org/nemo/Preview`，Nemo 静默吞掉，表现为「第一次按空格没反应」。主总线 `org.quicklook.Preview` 一直是先注册后抢名，所以没暴露；以后新增任何「由调用触发的服务名」都必须保持这个顺序。
20. **`GDK_BACKEND=x11` 会让所有 niri 规则失效**（表现为「默认浮动突然变默认平铺」）。XWayland 窗口在 niri 里的 App ID 是空字符串，`match app-id="^org\\.quicklook\\.Preview$"` 一条都命中不了，于是预览被平铺、抢不到焦点，`quicklook --status` 的 `backend` 会显示 `GdkX11Display`。这个变量常常来自 IDE / 自动化 shell 的环境，broker 由这种会话拉起（安装脚本、托盘重启）就会一路传给 viewer。现在 `service.rs` 的 `prefer_wayland()` 在有 `WAYLAND_DISPLAY` 时对所有子进程 `env_remove("GDK_BACKEND")`（viewer、托盘 spawn），`main.rs` 启动 `quicklook-settings` 同样处理。排查时先看 `--status` 的 `backend` 和 `niri msg windows` 的 App ID，别去改规则。
21. **默认布局是 niri 规则，不是客户端能决定的事**。Wayland 没有「请求浮动」的接口，`window_mode` 只能落到 `~/.config/niri/quicklook.kdl`。这个文件由 `core::niri::render/sync_at` 托管：规则正文每次按配置重写，文件尾部的 `binds`（安装器按需追加的 Ctrl+Alt+P）原样保留；`integration/niri.kdl` 与 Rust 模板由测试 `floating_template_matches_installed_integration` 钉住，改一处必须改两处，否则模板会漂移。niri 不一定会自动重载 include 的变更，所以同步之后要跑 `niri msg action load-config-file`（设置保存与 broker 启动都会做）。
22. **每个宿主的按键都要走 `Config::xxx_key()`，不要再读全局 `key()`**。Nemo 适配器用 `nemo_key()`、Thunar 适配器用 `thunar_key()`，`hosts.rs` 里 Nemo 的「是否装旁路器」判断用 `nemo_key()`，Yazi keymap 用 `yazi_key()`，Nautilus 脚本加速键用 `nautilus_key()`。漏改任何一处都会变成「设置里改了 Yazi 的键，实际改的是所有宿主」。另外两个适配器把按键缓存在进程内（`PREVIEW_KEY` / `OnceLock`），改键后必须重启对应文件管理器才生效——这是既有行为，文档也这么写。Caja 适配器同样只读 `caja_key()`，并且和 Thunar 共用 `core::keys::classify` 的按下/释放状态机，不要再复制一份判定。`classify` 比较按下/松开与 pressed 状态；HoldLatch 现在是布尔状态，没有 120 ms 超时。重新获得焦点时复位以处理发到预览窗口的 key-up，不能再把等待阈值当作长按检测。
23. **Caja 的私有符号在 `.dynsym` 里，但别把它当 Nemo**：发行版 `/usr/bin/caja` 导出了 `caja_view_get_type` / `caja_view_get_selection` / `caja_file_get_uri` / `caja_file_list_free`，所以 `dlsym(RTLD_DEFAULT, ...)` 直接可用，不需要 Nemo 那套虚表槽偏移，也不需要钉可执行指纹。反过来说，一旦某个构建没导出这些符号，适配器必须**一个旁路器都不装**（宁可不预览），不能退回到按偏移猜。
24. **`caja_view_get_selection` 是 transfer-full**：返回列表归调用方；本机元素实测是 GFile，也兼容 CajaFile/CajaFileInfo。按实际 GObject 类型取 URI，逐个 g_object_unref 再 g_list_free，不能把 GFile 传给 CajaFile 专属释放函数。
25. **Caja 的空格接管要覆盖五个 desktop 条目**：面板/菜单点的是 `caja-browser.desktop`，别的应用「打开文件夹」走的是 `caja-folder-handler.desktop`。只覆盖一个的现象是「有时能预览、有时不能」。清理时只在内容里带 `quicklook-caja` 才删，用户自己写的同名条目不能碰——卸载脚本对 Nemo / Thunar / Caja 的覆盖条目都已改成这个判定。
26. **Caja 右键没有 `.caja_action`**：那是 Nemo 的格式，Caja 不认。可用的是 python3-caja 扩展（`~/.local/share/caja-python/extensions/*.py`，加载器 `/usr/lib/*/caja/extensions-2.0/libcaja-python.so`）。写法照抄发行版自带的 `caja-folder-color-switcher.py`：类同时继承 `GObject.GObject` 与 `Caja.MenuProvider`，实现 `get_file_items(window, files)`；`do_get_file_items` 也一并留着，因为 caja-python 按属性名查、GObject 内省按 vfunc 名查，两条路径没法在这里同时目视验证。

## 十、待办与未决决策

**本轮已经落实的决策：**

1. 已决：新配置默认不接管按键，用户在独立接入区明确启用；已有配置保留原选择。
2. 已决：提供 deb；应用菜单包含独立设置入口，接入由桌面用户选择，不在包安装脚本里注入宿主。

**确定要做但没做的：**

- 图标是占位设计（蓝底白文档 + 白镜片），用户要求功能完成后重做正式版。
- `size_window()` 里 140px 的标题栏余量是硬编码魔数。
- `nemo-adapter` crate 参与两条路径：默认空格由 broker 的 `--nemo-compat` 处理；改键后由启动器加载适配器，用 key snooper 接管并吞掉裸空格。两条路径共用选择读取与 `Toggle` 逻辑，改其中一处要回归另一处。
- 从终端直接启动 `/usr/bin/nemo` 不经过启动器，改键后仍是原生空格行为；这一点无法在不改系统文件的前提下消除，文档已注明。
- 原生预览开关现在通过 ReloadConfig 同步到常驻 broker；应用/撤销会获取或释放 Nemo 名称，保持当前预览打开。
- PSD 图层树、epub、ics/vcf 未做；Thunar/PCManFM 之外的轻量文件管理器未适配。
- 用户可感知延迟（合成器出帧）没有独立测量。
- **desktop 条目覆盖的用户文件保护已补上**（2026-10-25）：`hosts.rs` 覆盖 `~/.local/share/applications/{nemo,thunar,caja*}.desktop` 前，若同名文件里不带我们的标记就先存一份 `<名字>.quicklook-original`，撤销接管时把它放回去（测试 `desktop_override_backs_up_and_restores_the_users_entry`）。卸载器同样只删带标记的条目。遗留边界：接管期间用户改的是**覆盖后**的文件，那份改动不会回到原条目上。
- 安装器备份按每文件保留最近一份；deb 不使用源码安装器的备份机制，用户级宿主覆盖的原文件另存 .quicklook-original。
- 设置窗口已增高到 760 px，首次显示将滚动位置归零；「恢复默认」修为真正默认选项，不再恢复成启动快照。

**验证缺口（代码写了但没在真机点过）：**

- Thunar 右键菜单项没有自动化点击验证（Wayland 注入不可用），只能用户手动验收。
- Caja 已在 Xephyr + marco + 私有总线里用真实 Caja 进程自动点验完整按键链路：空格开单选、长按只切一次、抬起丢失后自愈、多选网格、空选择不吞键、改键、右键菜单 provider 与菜单命令、未注入不接管、15 轮预览内存只涨约 1 MiB（证据见 `docs/VALIDATION.md`）。真机上还剩一次「用户从应用菜单重启 Caja」的目视确认——已在跑的实例没加载适配器，这一步只能由启动器弹窗或用户重启完成。终端改用 quicklook --launch-host caja；普通 caja 命令不再被覆盖。
- Thunar 4.18.8 已在 Xephyr 私有会话实测连续五组空格开/关；现有桌面上的旧实例仍需用户正常退出重开以加载新库。
- Nemo 空格与右键、Dolphin 右键已在 Xephyr + 私有会话总线上自动点击验证；Nemo 首次按空格的激活竞态用冷启动 5/5 复现验证。真实 Wayland 会话里 Nemo 原生 `ShowFile` 也已验证聚焦/悬浮/关闭。
- 改键路径已在 Xephyr + 私有总线上端到端验证：配置成 `Ctrl+Space` 后启动器加载适配器，`Ctrl+Space` 两次都打开预览窗口，裸空格不再打开任何窗口（证据见 `docs/VALIDATION.md`）。真机上的 Nemo 应用菜单重启仍需用户按一次确认。
- 托盘右键菜单已修并验证到 D-Bus 方法层，仍待用户在 noctalia 面板上目视确认。`scripts/uninstall-local.py` 已写好但尚未做一次真实卸载/重装演练。

## 十一、代码地图

| 文件 | 职责 |
|---|---|
| `crates/core/src/files.rs` | `Kind` 分类、`FileInfo`、自然排序、`classify()` |
| `crates/core/src/config.rs` | `Config`、XDG 路径、读写与夹取 |
| `crates/core/src/keys.rs` | `PreviewKey`（解析、keyval、X11 修饰位、yazi/GTK accelerator 格式化） |
| `crates/core/src/hosts.rs` | **所有文件管理器集成的唯一实现**：写/删托管文件、desktop 覆盖、`apply_icons`、`refresh_icon_cache` |
| `crates/core/src/protocol.rs` | 总线名、对象路径、D-Bus XML、`DETACHED_APP_ID` |
| `crates/core/src/niri.rs` | 托管 `~/.config/niri/quicklook.kdl`：按 `window_mode` 渲染浮动/平铺规则、保留尾部 `binds`、`reload()` 让 niri 重载 |
| `crates/daemon/src/main.rs` | viewer 入口、`--detached` / `--initial-uri`、注册 `org.quicklook.Viewer` |
| `crates/daemon/src/ui/mod.rs` | `Controller`：窗口、按键（无固定切换冷却）、D-Bus 方法、导航、分离窗口、状态 JSON |
| `crates/daemon/src/ui/views.rs` | 各 `Body` 变体的渲染（图、视频、文本、PDF、表格、Office…） |
| `crates/daemon/src/backend.rs` | `prepare()`：把 `FileInfo` 变成 `Body`；`Body`/`Prepared` 定义 |
| `crates/daemon/src/native.rs` | libvips / libarchive / libmpv 的 dlopen 封装，`archive_members`、`builtin_thumbnail` |
| `crates/daemon/src/office.rs` | OOXML 解析（docx/xlsx/pptx → HTML） |
| `crates/daemon/src/settings.rs` | 设置窗口（`quicklook-settings`） |
| `crates/daemon/src/worker.rs` | 两个解码线程池 |
| `crates/cli/src/main.rs` | 命令行参数 → D-Bus 方法 |
| `crates/cli/src/service.rs` | `quicklookd`：代理、按需拉起 viewer、托盘 |
| `crates/cli/src/tray.rs` | StatusNotifierItem + dbusmenu |
| `crates/cli/src/nautilus.rs` | `org.gnome.NautilusPreviewer2` 桥（`quicklook-nautilus-bridge`） |
| `crates/thunar-adapter/src/lib.rs` | Thunar 注入适配器（`g_application_run` + key snooper） |
| `crates/caja-adapter/src/lib.rs` | Caja 注入适配器：同 Thunar 的形状，符号从 `RTLD_DEFAULT` 查，选择列表逐项 g_object_unref 再 g_list_free |
| `crates/nemo-adapter/src/lib.rs` | Nemo 注入适配器（窗口信号 + `gtk_key_snooper_install`；列类型定位选中的 URI） |
| `scripts/install-local.py` | 事务化安装器：二进制、`.so`、图标、niri 配置、调用 `--apply-hosts`，失败按顺序回滚 |
| `scripts/uninstall-local.py` | 卸载器：`--dry-run`/`--purge`，先 `--quit` 再校验 pid 的 exe，剥离 niri/宿主托管块，最后检查残留 |
| `scripts/smoke-test.py` | 端到端冒烟（生成夹具 + 断言状态） |
| `integration/niri.kdl` | 三条窗口规则（弹窗浮动 / 分离窗口平铺 / 设置窗口浮动） |

## 十二、最后

排查这类问题最省时间的顺序是：先看 `quicklook --status` 的 JSON，再看 `~/.cache/quicklook/` 下的适配器日志，然后看进程 stderr（很多 GTK/GLib 警告只在那里），最后才去翻代码。

用户对「快」和「不打扰」很敏感：任何让窗口变慢、让主题不统一、让桌面行为被悄悄改掉的做法都要先问。

## 2026-10-26 后续修复

已删除固定 400 ms 冷却和 120 ms 计时锁存。Nemo / Caja / Thunar / Nautilus 46.4 / Yazi 的实际空格已验证，Thunar 和 Nautilus 的旧“未实测”条目以本记录为准。Nautilus 临时解包测试，没有做系统安装。Dolphin 23.08 仍只有右键，但 26.04 + KIO 6.24 可在宿主配置服务菜单快捷键；不能再泛称 Qt 无法接空格。新版 deb 为 0.1.0-2。

## 当前发布名称

用户指定版本为 V0.0.1。Cargo 与 deb 内部版本为 0.0.1，命令行、设置标题、运行状态显示 V0.0.1；发布包为 quicklook_V0.0.1_amd64.deb。此前 0.1.0-1 / 0.1.0-2 是开发过程记录，不再作为当前发布版本。

## 登录后 Thunar 空格失效修复（2026-10-26）

实际故障是 niri 的 Mod+E 执行 prefix/bin/launch-file-manager，脚本直接启动未适配的 thunar；预览服务正常，设置也没有丢失。此前只验证菜单入口与预置 LD_PRELOAD 的宿主，没有覆盖该启动脚本。

源码安装的 --with-niri 现在迁移已知脚本命令，保留输入法/Wayland 环境、参数和原命令回退；卸载移除托管块，保留用户修改。Thunar 接入另补两个专用 D-Bus 激活入口与用户 thunar.service 的 ExecStart 配置，撤销时恢复原文件，不接管默认通用文件管理器接口，也不重启当前窗口。

95 项 Rust 测试、安装/重复安装/卸载、脚本真实执行、全新进程的适配启动器/菜单/D-Bus 激活和连续空格均通过。用户已确认当前会话恢复。V0.0.1 应用版本不变，修复包为 quicklook_V0.0.1-1_amd64.deb；原 V0.0.1 标签不移动。

## Caja 普通命令启动支持（2026-10-26）

用户通过终端输入 caja 启动，旧版启动器覆盖没有覆盖这个入口。现在复用 python3-caja 用户扩展，在模块导入时通过 ctypes 调用 Rust 库的 quicklook_caja_install；加载不依赖预先点击右键菜单。Rust 核对进程与接管配置，再安装同一个 GTK 按键处理器；共享 GtkSettings 标记避免两份库同时安装重复回调，模块保持驻留。没有 python3-caja 时显式启动器继续可用。

本机已有 python3-caja 1.26.0，未新增系统包，也没有同名 PATH 替换。用户已确认普通 caja 命令恢复。当前 134 项 Rust 测试；原始命令、菜单和双加载连续按键测试与扩展方式完整 Caja 回归通过。当前应用版本为 V0.0.9，包文件为 quicklook_V0.0.9_amd64.deb（托盘右键新增「关于」并显示版本号）。
