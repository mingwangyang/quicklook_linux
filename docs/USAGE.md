# 使用指南

面向使用者的完整说明：怎么装、怎么用、每个键做什么、每个文件管理器能到什么程度、出问题怎么看。想了解实现取舍看 [IMPLEMENTATION.md](IMPLEMENTATION.md)，想核对验证范围看 [VALIDATION.md](VALIDATION.md)。

## 适用环境

- 目标：Ubuntu 24.04 / Linux Mint 22.x，amd64，niri 或其他 Wayland 合成器。
- 图形栈：GTK4；编解码走系统库（libvips、libmpv、Poppler、libarchive、WebKitGTK）。
- 不依赖 Electron，不需要 Wine。
- 程序不依赖 niri：GTK4 画窗口、D-Bus 传指令，Wayland 与 X11 都能跑，差别只在窗口布局策略。

### 实测过的桌面环境

| 环境 | 状态 |
|---|---|
| niri 26.04 + Wayland（Linux Mint 22.3） | **主力验证环境**，全功能真机验收，包括窗口规则与托盘（noctalia 面板） |
| X11 + KWin（KDE Plasma） | X11 预览窗口置顶（0.0.9）已实测；该会话下的文件管理器接入未测 |
| Nemo 6.6 / Caja 1.26 / Thunar 4.18 / Nautilus 46.4 / Dolphin 23.08 | 各宿主的空格/右键链路在 Xephyr 隔离会话用真实进程实测（Dolphin 仅右键） |
| GNOME / KDE / Cinnamon / MATE / Xfce 普通桌面 | 代码路径已验证（无平铺规则、开箱即用），未逐桌面目测 |
| sway / Hyprland | 平铺合成器：可识别并打印窗口规则片段供粘贴，未实机会话验证 |
| 托盘宿主 | noctalia 实测；KDE 面板、GNOME AppIndicator、Waybar 未测 |

完整验证记录（每项怎么测的、截图与数据）见 [VALIDATION.md](VALIDATION.md)。

## 安装

### deb 包（推荐，系统级）

~~~bash
sudo apt install ./dist/quicklook_V0.0.9_amd64.deb
~~~

安装后 `quicklook` 在 `/usr/bin`，应用菜单里会出现「Quick Look」和「Quick Look 设置」。库依赖由系统包管理，deb 不会自带 GTK、mpv、Poppler 这些运行时。

装完基本不用再做什么：安装脚本会以**发起安装的用户**身份自动补上它自己托管的那些文件。

- 你之前在设置里启用过接管（`hijack_native = true`）：升级后会自动重写文件管理器接入与窗口规则，等于自愈。
- 没有配置或没启用接管：只维护窗口规则（让预览浮在中间），**不会**去抢宿主的空格，也不会往宿主菜单里塞东西。

想手动控制的话：

~~~bash
quicklook --settings            # 图形界面：勾选接管后点「应用文件管理器接入」
quicklook --apply-hosts         # 按现有配置重写接入（幂等）
quicklook --apply-hosts --dry-run   # 只看会改哪些文件
quicklook --remove-hosts        # 撤销全部接入（卸载时也会自动执行）
~~~

### 源码安装（用户级，免 sudo）

~~~bash
source "$HOME/.cargo/env"
cargo build --locked --release --workspace
python3 scripts/install-local.py --with-niri --with-file-managers
~~~

装到 `~/.niri-stack/prefix`（可用 `QUICKLOOK_PREFIX` 改），不碰 `/usr`。niri 配置先校验再写入，变更与备份记在 `.local/dev-environment/local-install-*.json`。

两种形态不要同时装：用户级 desktop 文件与 D-Bus 服务优先级高于系统文件，会让命令指向旧版本。切换前先在设置里撤销接入，再卸载另一个。

## 第一次使用

装好后有三个入口，随便挑一个：

1. **文件管理器里按空格**——需要先在设置里启用接管（见下文「接管」），默认不接管。
2. **应用菜单 → Quick Look**，或 niri 里按 `Ctrl+Alt+P`：弹出原生选文件窗口。
3. **命令行**：`quicklook /path/to/file`。

~~~bash
quicklook /path/to/file        # 开关语义：已显示同一个文件时关闭
quicklook --show a.png b.pdf   # 始终显示；多文件进缩略图网格
quicklook --pick               # 选文件窗口
quicklook                     # 同上
echo -e "a.txt\0b.png\0" | quicklook --stdin0
~~~

**接管是一次性选择。** 新配置默认 `hijack_native = false`，也就是装完不会自动抢宿主原有的空格。要空格预览，打开设置窗口勾选「启用预览按键接管」、选好文件管理器，再点「应用文件管理器接入」。已有配置会保留你上次的选择。

## 预览窗口里的按键

窗口打开后点一下让它拿到焦点，然后：

| 键 | 作用 |
|---|---|
| `Space` / `Esc` | 关闭（全屏时 `Esc` 先退出全屏；在压缩包内部时先退回上一层） |
| `↑` `↓` | 上一个 / 下一个文件（同一目录内自然排序） |
| `Enter` | 用默认应用打开当前文件 |
| `Backspace` | 从包内文件退回上一层 |
| `Ctrl+D` | 分离成独立窗口 |
| `Ctrl+C` | 复制文件；若已选中文本则复制文本 |
| `F` | 全屏 / 退出全屏 |
| `P` | 固定窗口（固定后 `↑↓`、`Space` 不再关闭） |
| `I` | 属性侧栏 |
| `T` | PDF 缩略图侧栏 |
| `Page Up` / `Page Down` | PDF 上一页 / 下一页 |

按内容类型额外可用：

- **图片**：滚轮或捏合缩放，拖动平移，双击或 `0` 适应窗口，`1` 原始像素，`R` 顺时针旋转、`Shift+R` 逆时针。
- **PDF**：拖动选字，`Shift`+拖动平移；侧栏里可跳页与切连续滚动。
- **视频 / 音频**：`K` 暂停，`,` `.` 逐帧，`<` `>` 调速；音频有封面、波形、频谱三种视图。
- **文本 / 代码**：`W` 自动换行，`E` 循环编码（UTF-8 / GBK / GB18030 / Windows-1252），大文件分段继续加载。
- **Markdown**：可在渲染视图与源码之间切换；含 ```mermaid 代码块时用内置 Mermaid 渲染流程图。

**窗口分离（`Ctrl+D`）** 值得单独说：当前预览会变成独立进程的窗口，默认平铺，可以长期开着，关掉它之前和之后都能继续用空格开新预览。独立窗口有自己的缓存和状态，互不影响；它的标题后面带「（独立窗口）」。

## 各文件管理器的实际能力

完整矩阵与验证细节见 [FILE_MANAGERS.md](FILE_MANAGERS.md)。这里只讲使用上的差别。

| 文件管理器 | 空格预览 | 多选 | 备注 |
|---|---|---|---|
| Nemo | 支持 | 右键传完整多选 | 改键后需要重启 Nemo；从终端直接 `nemo` 不经过启动器，仍用空格 |
| Thunar | 支持 | 右键传完整多选 | 靠 LD_PRELOAD 适配器；登录后自动启动的实例也带适配 |
| Caja | 支持 | 右键传完整多选 | 需要 `python3-caja`，由用户扩展加载适配器 |
| Nautilus | 宿主的原生预览协议 | 脚本入口传完整多选 | 裸空格走宿主自己；只有 `Ctrl`/`Alt` 组合键能写进脚本加速键 |
| Dolphin | ❌ 目前无法支持空格预览（仅右键） | 支持 | 空格是 Dolphin 自己的「打开/重命名」行为；23.08 没有预览热键接口，26.04+ 需 KIO ≥ 6.24 才能在宿主里手动绑定 |
| Yazi | 支持 | 光标跟随 | 默认空格，`Esc` 关闭且保留原有 `Esc` |

**为什么不是每个都能改键**：Nautilus 的空格由它自己处理，外部只能写脚本加速键；**Dolphin 目前无法支持空格预览**——按空格是它自己的「打开/进入」行为，本项目没有任何公开接口能接管它（23.08 没有预览热键接口；26.04+ 的服务菜单快捷键要求 KIO ≥ 6.24，且那也是用户在 Dolphin 里手动绑定的，不是本工具接管）。改键后都要重启对应的文件管理器，因为按键在进程启动时读一次。

**接管做了什么**：Nemo / Thunar / Caja 通过启动器加载一个 `.so`（`LD_PRELOAD`）旁路按键；Nautilus 通过 `org.gnome.NautilusPreviewer2` 接口；Yazi 改 `keymap.toml`；Dolphin 只写服务菜单。全部写在你的用户目录（`~/.local/share`、`~/.config`），不需要 root，不修改 `/usr`，也不替换终端里的同名命令。

## 设置窗口

~~~bash
quicklook --settings          # 打开设置窗口
quicklook --apply-hosts       # 按当前配置重写集成，不开窗口
quicklook --remove-hosts      # 移除所有由 Quick Look 写入的集成
quicklook --apply-hosts --dry-run   # 只列出会改哪些文件
~~~

窗口分六组：

1. **预览键**——全局默认（`Space` / `Return` / `F1`–`F12`，可叠加 `Ctrl`/`Shift`/`Alt`），并可为单个文件管理器单独指定。留空表示跟随全局。
2. **窗口**——默认布局浮动或平铺；独立窗口（`Ctrl+D`）始终平铺。
3. **启动**——是否登录时自动启动常驻服务。
4. **外观**——浅色 / 深色主题；是否渲染 Markdown 里的 Mermaid 流程图。
5. **文件管理器接入**——接管总开关 + 六个宿主勾选。
6. **渲染与缓存**——图片最大边长、缓存上限、预览进程空闲保留时间。

**关于「登录时自动启动常驻服务」**：默认关闭。打开后会在 `~/.config/autostart/` 写一个 `org.quicklook.Preview.desktop`，登录时启动常驻服务 `quicklookd`（提供通知栏图标，并把首次预览预热到几十毫秒）。预览窗口本身不随登录启动，仍然是按需打开、用完退出。关掉开关再保存就会删除这个条目；卸载时也会一并清掉。想临时不让它启动，也可以直接用桌面环境自带的「启动应用程序」工具停用——那是同一个文件。

「保存设置」只写配置；**接入需要另外点「应用文件管理器接入」**。这两个动作分开是有意的：改主题不该顺手去动宿主配置。

## 配置文件

路径 `~/.config/quicklook/config.toml`（尊重 `XDG_CONFIG_HOME`），缓存默认在 `~/.cache/quicklook`。

| 字段 | 默认 | 说明 |
|---|---|---|
| `preview_key` | `"Space"` | 全局预览键 |
| `preview_key_nemo` 等 | 空 | 单个宿主覆盖，空串跟随全局 |
| `theme` | `"light"` | `light` / `dark` |
| `mermaid` | `true` | 是否渲染 Markdown 里的流程图 |
| `window_mode` | `"floating"` | `floating` / `tiled` |
| `hijack_native` | `false` | 接管总开关 |
| `autostart` | `false` | 登录时自动启动常驻服务（写 XDG autostart 条目） |
| `host_nemo` 等六项 | `true` | 各宿主是否接入（受总开关约束） |
| `cache_mb` | `200` | 预览缓存上限 |
| `image_max_edge` | `2560` | 图片解码最大边长 |
| `idle_timeout_secs` | `600` | 常驻服务空闲退出时间 |
| `viewer_grace_secs` | `3` | 预览进程关闭后保留秒数 |
| `text_chunk_bytes` | `262144` | 文本首段大小 |
| `max_text_buffer_bytes` | `8388608` | 文本上限 |
| `max_archive_entries` | `10000` | 压缩包列出条目上限 |
| `max_extracted_bytes` | `134217728` | 单次解压上限 |

改成非法值时不会崩，会退回安全默认（例如无法解析的预览键按空格处理）。

## 支持的格式

| 类别 | 覆盖 |
|---|---|
| 图片 | PNG、JPEG、BMP、SVG、ICO；TIFF、HEIC/HEIF、AVIF、JXL（靠 libvips 模块） |
| 动画 | GIF、WebP（小动画逐帧；超长转视频或双帧队列） |
| 视频 / 音频 | libmpv 支持的容器与编码 |
| 文档 | PDF、AI（含兼容内容）、EPS |
| Office | docx / xlsx / pptx（原生 OOXML 解析，不做版式还原） |
| 文本 / 代码 | 按编码探测，GtkSourceView 高亮 |
| 富文本 | Markdown（含 Mermaid）、HTML、RTF |
| 结构 | JSON、XML、YAML 折叠树；JSON 默认落在源码页 |
| 表格数据 | CSV、TSV（按分隔符渲染成表格，可切源码；数值列右对齐） |
| 压缩包 | ZIP、TAR、7z、RAR；包内可继续展开，最多 4 层 |
| 其他 | 目录列表、字体、RAW（CR2/NEF/ARW 等）、PSD 合成图、二进制十六进制摘要 |

## 托盘图标

常驻的 `quicklookd` 会注册一个 StatusNotifierItem 图标（蓝底「文档 + 放大镜」，浅深色主题一致）：

~~~text
左键      打开选文件预览
右键菜单   设置… / 关于 Quick Look / 重启 / 退出
~~~

右键「关于 Quick Look」弹出一个只读小窗，显示应用名与当前版本号。

宿主没有托盘区时不显示，但不影响任何预览功能。托盘是可选入口。

## 命令行参考

| 参数 | 作用 |
|---|---|
| `PATH...` | 要预览的文件；默认开关语义 |
| `--show` | 显示而不是切换关闭 |
| `--pick` | 打开选文件窗口 |
| `--next` / `--prev` | 在同一目录序列里前后移动 |
| `--close` | 关闭当前预览 |
| `--back` | 从包内文件退回上一层 |
| `--pin` / `--unpin` | 固定 / 取消固定 |
| `--page N` | 跳到 PDF 第 N 页（从 1 开始） |
| `--extract N` | 预览压缩包第 N 个条目（从 0 开始） |
| `--status` | 打印当前状态 JSON |
| `--quit` | 退出常驻服务 |
| `--set-selection` | 只更新已打开的预览，关闭时不重弹 |
| `--from-nautilus` | 读取 Nautilus 脚本传入的 URI |
| `--stdin0` | 从标准输入读 NUL 分隔路径 |
| `--launch-host <caja\|thunar\|nemo>` | 用已启用的适配启动器打开该文件管理器 |
| `--settings` | 设置窗口 |
| `--apply-hosts` / `--remove-hosts` | 应用 / 移除集成，`--dry-run` 只看不改 |
| `--screenshot PNG` | 保存当前预览截图（验证用） |

## 常见问题

**按空格没反应。** 先确认接管已启用并应用过（设置 → 文件管理器接入 → 应用），再确认该文件管理器是接管后启动的（重启一次）。Nemo 从终端直接启动不经过启动器，走的是它自己的空格。

**改了预览键但没生效。** 按键在文件管理器启动时读取一次，必须重启该宿主。Nautilus 只接受带 `Ctrl`/`Alt` 的组合。

**`quicklook` 命令报连不上服务。** 常驻服务没在跑。正常会自动拉起；手动启动可运行 `quicklookd &` 或 `quicklook --status` 触发自动激活。

**预览窗口打开了但马上消失。** 检查是否被固定（`P`）以外的逻辑关闭，或预览进程空闲退出时间被设得很小（`viewer_grace_secs`）。

**托盘没有图标。** 宿主没有 StatusNotifierWatcher 时不会显示；如果其它应用有托盘而你没有，重建一次图标缓存：`gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor`。

**Markdown 里流程图只显示源码。** 检查设置里「渲染 Markdown 里的 Mermaid 流程图」是否开启（配置项 `mermaid`）；只识别 ```mermaid 围栏代码块。

**BMP / AVIF 打不开。** 这两类依赖 libvips 的 magick / heif 模块。程序对常见的 PNG/JPEG/GIF/WebP/BMP/ICO/TIFF 有内置解码兜底，模块缺失时会自动回退并在状态栏说明。

**全屏后按 `F` 退不出。** 已修复合成分器侧全屏（例如 niri 的 `Mod+F`）与程序状态不同步的问题；若仍复现，`Esc` 会先退全屏而不是关窗口。

**想彻底恢复原状。** `quicklook --remove-hosts` 移除托管文件与托管块，并恢复备份的宿主入口，不关闭正在浏览的窗口。源码安装再跑 `python3 scripts/uninstall-local.py`；deb 安装用 `sudo apt remove quicklook`，程序不会清除你的个人配置。

## 卸载

~~~bash
# deb
quicklook --remove-hosts
sudo apt remove quicklook

# 源码
quicklook --remove-hosts
python3 scripts/uninstall-local.py
~~~

两者都不会主动清除 `~/.config/quicklook` 与 `~/.cache/quicklook`，重装可继续用。已加载进正在运行的文件管理器的适配器，会在该进程正常退出后释放。

## 性能预期

本机（Linux Mint 22.3 / niri / Wayland）实测：预览进程首次启动约 340 ms，短时间关闭重开中位约 19 ms，缓存切换中位约 18 ms，万行文本首段约 34 ms，空闲常驻约 6.5 MiB。108 MP PNG 首轮约 1.4–2 秒，放大后按视口补细节。

这些是请求到数据/控件就绪的时间，**不含合成器出帧**，也不是所有格式的保证。
