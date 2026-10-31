# 使用与实现说明

## 架构

| 模块 | 职责 |
|---|---|
| crates/core | 文件 URI/MIME、自然排序、配置、编码分段读取、DBus 协议；统一预览键与宿主集成落地（keys.rs / hosts.rs） |
| crates/cli | quicklook 和轻量 quicklookd；常驻部分只依赖 GIO |
| crates/daemon | 按需启动 GTK4 预览窗口；关闭清空缓存，默认 3 秒后退出。另有 quicklook-settings 设置窗口 |
| crates/clipboard | 独立轻量 Wayland 剪贴板持有程序，提供 GNOME/KDE 文件 MIME，关闭窗口后仍可粘贴 |
| crates/nemo-adapter | Rust 动态库，读取 Nemo 真实多选与文件视图按键；GTK3 留在 Nemo，GTK4 在独立进程。默认空格由 broker 的原生 `org.nemo.Preview` 处理；改键后启动器加载本库，用 GTK key snooper 在视图消费按键前接管 |
| crates/caja-adapter | Caja（MATE）的空格旁路器，与 Thunar 适配器共用 `core::keys` 的按键状态机；用 `dlsym(RTLD_DEFAULT, ...)` 查 Caja 自己 `.dynsym` 里的视图与选择符号，符号查不到就不装旁路器。选择列表按 transfer-full 交给 `caja_file_list_free` 释放 |

后台线程负责解码、目录统计、压缩包读取。当前文件优先，邻近 ±3 项预取，切换取消旧任务，代次检查防止旧结果覆盖新文件。图片/PDF 通过 GDK 纹理进行 GPU 合成，视频采用 libmpv Render API / GtkGLArea，首选硬解，不支持时降级。图片/PDF 内容解码仍需要 CPU。

窗口分离（Ctrl+D）不再新开同进程窗口：viewer 只持有一个内容盒子和一个 DBus 对象，开第二个窗口需要重构渲染层。改为给兄弟二进制传 `--detached --initial-uri=<uri>`，由它自己起一个进程，app-id 是 `org.quicklook.Preview.Detached`。这个 id 不匹配弹窗那条 niri 规则，所以新窗口按普通窗口平铺；它不请求 `VIEWER_BUS`，也不注册 DBus 对象，关掉只结束自己。原窗口随即 `close()` 并按空闲秒数回收，因此分离后仍能按空格开新预览。分离窗口的 `detached` 状态让 Enter 打开外部应用时不自动关窗。

统一预览键放在 `config.preview_key`（默认 `Space`，可选 `Space`/`Return`/`F1`–`F12` 加 Ctrl/Shift/Alt）。不用字母键是刻意的：字母的 GDK keyval 随 Shift 层变化，适配库直接比对 keyval 会歧义。适配库进程内读一次配置，按下修饰位被子集判断后才拦键，其余按键照旧交还宿主。

各宿主对改键的跟随程度不同：Nemo / Thunar / Yazi 跟随；Nemo 默认空格仍走原生 `org.nemo.Preview`，只有非默认键才生成启动器、加载适配器并把裸空格吞掉；Nautilus 的裸空格归宿主，只有 Ctrl/Alt 组合能写进脚本加速键；Dolphin 目前无法支持空格预览：空格是宿主自己的打开/重命名行为，23.08 没有预览热键接口（26.04 + KIO 6.24 起才能在宿主手动绑定）。改键后必须重启对应宿主。

外观由 `config.theme`（`light` / `dark`，默认浅色）统一控制：viewer 启动时设置 GTK 深色偏好并选择 `style.css` 或叠加 `style-dark.css`；WebKit HTML 在根节点加 `dark` 类，配色全部走 `web.css` 的 CSS 变量；SourceView 分别选 `Adwaita` / `Adwaita-dark`。设置窗口保存后立即切换自身主题，新开的预览窗口读取新配置。

`hosts.rs` 把设置落成各宿主的用户级文件，只改自己写过的托管标记块（`# BEGIN/END quicklook managed integration` 与 Lua 的 `--` 版本），撤销时连块一起删，重复执行幂等。设置进程与 `quicklook --apply-hosts` 走同一实现，因此界面和命令行结果一致。

原生 Wayland 窗口由 niri 规则控制浮动与非激活。Wayland 客户端不能规定全局焦点策略，其他合成器需要对应规则；无焦点按键由 Nemo 适配转发。Nemo、Dolphin、GNOME Files 和 Yazi 接入方式见 FILE_MANAGERS.md。没有文件管理器时可用 --pick 或不传参数打开原生选文件窗口，CLI/DBus 不依赖任何宿主。

## 格式和边界

| 格式 | 实现 |
|---|---|
| JPG/PNG/BMP/SVG/ICO、TIFF/HEIC/HEIF/AVIF/JXL | libvips 有界纹理，静态大图支持原始尺寸坐标和视口细节补解码，EXIF 方向校正 |
| GIF/WebP | 小动画最多 240 帧/96MiB；长 GIF 走 libmpv，长 WebP 使用双帧队列，支持暂停 |
| 视频/音频 | libmpv，进度/音量/静音/倍速/逐帧/循环；封面/波形/频谱 |
| PDF/PDF 兼容 AI | Poppler 单页/连续、缩略图、跳页、文本选择；加密 PDF、无兼容内容的 AI 显示错误 |
| 文本/代码 | GtkSourceView 高亮/行号；UTF-8/GBK/GB18030/Windows-1252，支持 BOM 和多字节分页边界 |
| Markdown/HTML/RTF | WebKitGTK 渲染，Markdown/RTF 可切源码；RTF 基础样式；关闭脚本、远程资源、Web 存储。Markdown 里的 ```mermaid 流程图用内置 Mermaid 渲染，见下节 |
| JSON/XML/YAML | 惰性折叠树/源码，每层前 500 项，深层/大值提示查看源码；JSON 默认打开源码页 |
| CSV/TSV | 扩展名/MIME 加内容嗅探确定分隔符，RFC 4180 解析，HTML 表格（粘性表头、行号、数值右对齐），可切源码；上限 2000 行 × 60 列 |
| 目录 | 列表、异步递归统计；最多 10 万项/3 秒/64 层，注明部分统计，不跟随符号链接 |
| ZIP/TAR/7z/RAR | libarchive 时间/大小列表，按需解压普通文件，拒绝路径穿越和链接；包内的压缩包可继续展开，最多 4 层 |
| Office（docx/xlsx/pptx） | 直接解析 OOXML：docx 取段落/标题/列表/表格，xlsx 取工作表与单元格，pptx 取放映顺序与文本；不调用 LibreOffice，不做版式还原 |
| 二进制 | MIME/属性、前 2KiB hex 与可打印字符串摘要 |
| CR2/NEF/ARW/DNG | LibRaw 内嵌 JPEG 优先，无缩略图时完整解码；64MP 原始尺寸及输出保护，失败再尝试 vips |
| PSD | Rust 跳过资源和图层，读 8-bit RGB/灰度 RAW/RLE 合成图；失败交给 vips，不展示图层树 |
| EPS | Ghostscript 安全模式转 PDF，按文件元数据缓存，有超时与输出限制 |
| 字体 | ttf-parser 元数据/示例，Fontconfig 只注册到本进程 |

RAW/PSD 缩放针对合成/内嵌预览，不保证原始传感器像素；相机型号依系统 LibRaw。大图先显示缩小预览，再补充放大区域；大 PNG 仍需顺序读取。动画超过像素/输入限额时提示或降级静态预览。HTML 本地相对图片取决于 WebKit 文件访问限制。

文本先读 256KiB，可继续加载至 8MiB；结构/HTML/RTF 首段至少 2MiB。截断或解析失败显示源码。Markdown 继续加载更新源码，完整文档可用默认编辑器打开。

## Markdown 流程图（Mermaid）

Markdown 里的 ```mermaid 代码块用内置的 Mermaid 渲染成图，支持 flowchart、sequence、class、state、gantt 等 Mermaid 11 全部图型。

**为什么要开 JavaScript。** 预览的 WebView 默认 `set_enable_javascript(false)`，Mermaid 无法运行。两个候选方案：在 Rust 里自己实现布局，或让 Mermaid 在页面里跑。自己实现只能覆盖 flowchart 的子集，而且要处理连线交叉、标签测量、自动布局，成本远高于收益，所以选后者，但把风险面收到最小：

- **只有确实含 mermaid 代码块的 Markdown 视图才开 JavaScript**，判断来自 `mermaid::has_diagram()`（扫围栏代码块的信息串）。其它视图（HTML、Office、普通 Markdown）仍然完全关闭。
- Mermaid 由 **WebKit 用户脚本**注入，不来自文档内容：`UserScript` 在 `DocumentStart` 注入 bundle，在 `DocumentEnd` 注入引导脚本。用户脚本由引擎直接注入，不受页面 CSP 约束——这是能保持 CSP 严格的前提。
- **页面 CSP 保持 `default-src 'none'`**，不含 `script-src`，所以 Markdown 正文里手写的 `<script>` 依然不会执行。
- 代码块文字用 `textContent` 取出再交给 Mermaid，不经过 HTML 解析；Mermaid 用 `securityLevel: 'strict'` 过滤标签里的 HTML。
- 渲染结果通过 `window.webkit.messageHandlers.quicklookMermaid` 回报 `ok:N` / `error:…` / `none` / `unavailable`，写进 `--status` 的 `mermaid` 字段，便于自动化断言和排查。

**为什么把 bundle 压成 gzip 入库。** 未压缩的 Mermaid 11 是 3.4 MiB，`include_bytes!` 内嵌会让 `quicklook-viewer` 直接胖 3.4 MiB；`gzip -9` 后约 950 KiB，运行时首次遇到流程图解压一次并缓存在 `OnceLock` 里。代价是首次渲染多几毫秒，换来二进制小 2.5 MiB。

**主题。** `theme` 配置同时决定 Mermaid 的 `theme`（`default`/`dark`）和 `themeVariables.edgeLabelBackground`——后者必须显式对齐，否则深色主题下连线标签会是浅色小方块。

**关闭方式。** 设置里的「渲染 Markdown 里的 Mermaid 流程图」，或配置 `mermaid = false`。关闭后 mermaid 代码块按普通代码块显示，源码视图始终可用。

## CSV / TSV 表格预览

纯文本视图里 CSV 列不对齐、表头埋在数据里，几百行就看不出结构，所以单独渲染成表格：

- **识别**：扩展名 `.csv` / `.tsv` / `.tab` / `.psv`，或 MIME `text/csv` / `text/tab-separated-values`；再按内容嗅探分隔符（`,` `;` `\t` `|`）——Excel 导出的 CSV 常是分号或制表符，只看扩展名会解析成一列。
- **解析**：RFC 4180 状态机，支持引号包裹、`""` 转义、字段内换行、CRLF、UTF-8 BOM；行尾空行丢弃；短行按最长列补齐。
- **渲染**：HTML + WebKit，复用 `web.css` 的主题变量，所以深色主题自动跟随。首行作表头，左侧行号，表头与行号 `position: sticky`；逐列判断是否数值（允许千位分隔符与百分号）后右对齐并使用等宽数字；斑马纹。
- **上限**：最多渲染 2000 行 × 60 列，单元格超过 400 字符截断；读取上限 4 MiB（配置的 `text_chunk_bytes` 不足时抬到 4 MiB）。超出时底栏写「仅读取前 …」「仅显示前 2000 行（共 N 行）」。
- **两种模式**：格式栏的「表格 / 源码」由 `GtkStack` + `StackSwitcher` 提供，源码页是 GtkSourceView，可按 E 切换编码（中文 CSV 常是 GBK，`chardetng` 一般能自动认出来，认错时手动切）。
- **JSON 默认源码**：结构视图本来就有「结构 / 源码」两页，现在 `.json` / `.jsonc` 默认落在源码页——读 JSON 多数时候是想直接看原文；XML、YAML 仍默认结构树。

为什么不自己画表格：2 万单元格的 GTK 列表要自己实现虚拟化与列宽记忆，而 WebKit 已经有粘性表头、文本溢出省略和主题变量，和 Office 表格用的是同一套样式，视觉也统一。

## Office 与包内浏览

Office 预览不调用 LibreOffice，也不走 pandoc，而是直接读 OOXML。原因是 docx/xlsx/pptx 本身就是「ZIP + 若干 XML」：解析 document.xml、workbook.xml + sharedStrings.xml + worksheets、presentation.xml + slides 只需要一次 libarchive 读取和几遍 roxmltree 遍历，冷启动在 10ms 量级，而 headless 转换即使有缓存也要数百毫秒到数秒，并且要在磁盘上留一份转换产物。代价是不做版式还原：字体、页边距、分栏、图片、图表、SmartArt 都不显示，只保证「文字、标题层级、列表、表格、工作表、放映顺序」可读。xlsx 的日期单元格按序列号原样显示（需要 numFmt 才能还原成日期），这是有意为之的取舍，文档里已注明。

解析结果生成语义化 HTML，交给预览器已有的 WebKit 视图渲染。该视图关闭 JavaScript、禁用 Web 存储、CSP 限制为 `default-src 'none'`，只允许内联样式，因此文档里的超链接、图片引用和外部资源都不会真正加载，也不需要额外写一套 GTK 表格控件。单元格由 `<table>` 布局，粘性表头由 CSS `position:sticky` 完成。

规模上限：docx 最多 6000 段、表格 400 行 × 32 列；xlsx 最多 60 个工作表、每个 400 行 × 32 列；pptx 最多 60 张；包内 XML 总量 64MiB，生成 HTML 上限 8MiB。触发上限时在状态行给出提示，而不是静默截断。

包内浏览用一个栈保存每一层的 `Prepared`。进入包内文件时把外层压栈，`Backspace` / `Esc` 弹栈回到上一层，标题显示 `a.zip › b.tar › hello.txt` 的面包屑。最多展开 4 层，防止压缩包套娃把暂存区吃光。按下 Esc 时如果栈非空就只回退一层，栈空才关闭窗口——这样「一路 Esc 退出」符合直觉，又不会一下子关掉整个预览。包内文件被解压到 `~/.cache/quicklook/extracted` 下的临时目录，随 `Prepared.temporary` 在离开该层时清理。

Office 与包内浏览可以叠加：zip 里的 docx 直接按 Office 渲染，验证时截图为 `bundle.zip › notes.docx`。

## 配置

~/.config/quicklook/config.toml，修改后运行 quicklook --quit：

~~~toml
cache_mb = 200
image_max_edge = 2560
idle_timeout_secs = 600
viewer_grace_secs = 3
text_chunk_bytes = 262144
max_text_buffer_bytes = 8388608
max_archive_entries = 10000
max_extracted_bytes = 134217728
preview_key = "Space"
preview_key_nemo = ""
preview_key_thunar = ""
preview_key_yazi = ""
preview_key_nautilus = ""
theme = "light"
window_mode = "floating"
hijack_native = true
host_nemo = true
host_dolphin = true
host_nautilus = true
host_yazi = true
nemo_compat = false
~~~

按键解析集中在 `keys::PreviewKey`（只接受 `Space/Return/F1-F12` 加 `Ctrl/Shift/Alt`，字母 keyval 会随 Shift 层变化所以不支持）。`Config::key()` 是全局默认，`nemo_key()` / `thunar_key()` / `yazi_key()` / `nautilus_key()` 是各宿主的实际按键：覆盖为空或解析失败就跟随全局。写入侧一一对应——`apply_nemo` 用 `nemo_key()` 判断是否要装旁路器启动器，`apply_yazi_keymap` 用 `yazi_key()` 生成 `on = "<...>"`，`apply_accels` 用 `nautilus_key()` 生成 `scripts-accels`；读取侧 Nemo / Thunar 适配器在进程启动时各取自己的键并缓存（`PREVIEW_KEY` / `OnceLock`），所以改键要重启对应文件管理器。新增宿主入口时必须同时接上这两侧，否则会出现「设置里改了 Yazi，实际改了所有宿主」。

cache_mb 是解码缓存预算，纹理、WebKit、解码器还会另占内存。隐藏窗口回收后只留 broker，空闲 10 分钟 broker 退出。viewer_grace_secs 可设 0–30。

解压暂存位于 ~/.cache/quicklook/extracted/，单文件默认 128MiB，总量 512MiB 保护；下一次启动清理超过一天的本项目目录，保证默认应用/剪贴板在关闭窗口后仍可读取。EPS 位于 ~/.cache/quicklook/eps/，退出服务后可手动清理。

静态图解码优先走 libvips（有 shrink-on-load、方向自动校正和区域渲染），失败时退回 `image` crate 自带的解码器，覆盖 png/jpeg/gif/webp/bmp/ico/tiff。之所以要这层兜底：libvips 8.15 起不再自带 BMP 读取器，BMP 要靠 `vips-magick` 模块，模块缺失时 P0 级的 BMP/ICO 会直接打不开。兜底命中时状态行会说明「libvips 缺少该格式的解码器，已用内置解码器」，不会静默改变行为。AVIF/HEIC/JXL 只在 libvips 模块里存在，没有内置兜底。

## 窗口布局与空格重复

Wayland 客户端不能要求自己的窗口浮动，`window_mode` 只能落到 niri 的窗口规则上。`crates/core/src/niri.rs` 托管 `~/.config/niri/quicklook.kdl`：按配置渲染选择器 / 预览 / 独立窗口 / 设置四条规则，并把文件尾部原有的 `binds`（安装器按需追加的 Ctrl+Alt+P）原样保留，写完调用 `niri msg action load-config-file`。触发点有两处：设置保存，以及 broker 启动（覆盖手改 config.toml 与安装后首次启动）。没有这个文件（安装时未加 `--with-niri`）就什么都不做，设置里给一行提示而不是报错。`integration/niri.kdl` 与 Rust 模板由测试钉住，两者必须一起改。

前提是窗口真的以 Wayland 运行：XWayland 窗口在 niri 里 App ID 为空，所有 `app-id` 规则都命中不了，表现就是「默认浮动变成默认平铺」并且抢不到焦点。终端 / IDE 常会导出 `GDK_BACKEND=x11`，broker 由这种会话拉起时会一路传给 viewer，所以 `service.rs::prefer_wayland()` 与 `main.rs` 启动设置窗口时，在有 `WAYLAND_DISPLAY` 的前提下 `env_remove("GDK_BACKEND")`。排查顺序：`quicklook --status` 的 `backend`（应为 `GdkWaylandDisplay`）→ `niri msg windows` 的 App ID → 规则文件。

空格按一下像按了很多次，来源是键盘自动重复：Nemo 原生空格在自己的 GTK 里持续补发 `ShowFile`，Thunar/Yazi 与预览窗口自身的空格关闭也一样会被补发。适配器层的 `HELD_KEY` 去重只覆盖注入路径，管不到宿主原生总线，所以最后一道闸放在 viewer：`Controller::allow_toggle()` 记录最近一次切换时间，400ms 内的 `ShowMode::Toggle` 与预览窗口内的空格都直接丢掉。选 400ms 是因为键盘重复延迟通常 ≥500ms——长按最多再切一次，连续点按不受影响；`Show` 和 `SetSelection` 不加闸，多选与方向键实时选择不能被误伤。冷却只挡连击，不挡「用户关完又想开」：Esc、关窗按钮、`Close`、分离、交给默认应用五处走 `close_by_user()`，先清掉时间戳；只有切换自身驱动的关闭保留它。

## CLI 与 DBus

quicklook --help 查看参数。支持普通路径、file URI、多选、--stdin0 NUL 分隔文件名；只读取本机普通文件/目录。

公共总线及接口 org.quicklook.Preview，对象 /org/quicklook/Preview：

- Preview(as)、Show(as)、Toggle(as)、SetSelection(as)、Pick()。SetSelection 只更新已打开的预览，不会唤醒或重开已关闭窗口。
- Next()、Prev()、Close()、Pin(b)、Page(i)、Extract(u)。
- Action(s)：fullscreen、pin、rotate、rotate-left、fit、actual、pause、properties、wrap、encoding、thumbnails、continuous、page-prev、page-next。
- Status() 返回 JSON，Quit() 停止，Screenshot(s) 导出本程序 PNG（须完成绘制）。

org.quicklook.Viewer 为内部接口。可选 nemo_compat=true 提供旧 org.nemo.Preview（单 URI），默认关闭以避免冲突。

复制文件由独立 Rust Wayland 帮助程序同时提供 text/uri-list、x-special/gnome-copied-files 和 KDE 的复制标记，面向 Dolphin 与 GNOME/Nemo。合成器需要 data-control 协议；不支持时退回 wl-copy 的 GNOME 文件目标。已选文本仍优先复制。没有覆盖用户当前剪贴板做实测。

## 托盘图标

常驻的 `quicklookd` 在拿到 org.quicklook.Preview 之后，顺手把 `org.kde.StatusNotifierItem` 和 `com.canonical.dbusmenu` 两个对象挂到同一条连接上，总线名是 `org.kde.StatusNotifierItem-<pid>-1`，路径 `/StatusNotifierItem` 与 `/MenuBar`，再调用 watcher 的 `RegisterStatusNotifierItem` 登记。选 StatusNotifierItem 而不是老的 XEmbed 托盘，是因为 Wayland 下没有 XEmbed，niri + noctalia 这条链路只认 SNI。

放在 broker 而不是预览进程：预览进程有 3 秒空闲就退出，托盘图标会跟着闪没；broker 是常驻的，生命周期正好对得上。

图标用应用自己的彩色图标（`org.quicklook.Preview`，蓝底白色文档 + 放大镜），不用主题图标名。图形刻意不是单纯的放大镜——单独一个镜片在面板里和「搜索」无法区分，配上文档轮廓才读得出是预览。这版是占位设计，正式图标等交互收尾后再做。理由有两条：一是颜色自己说了算，图标带底板，浅色与深色主题下观感一致，不必依赖宿主把 symbolic 图标重新着色、也不必依赖它以 -dark 主题重新解析名字；二是和桌面条目、窗口图标同源，用户在两处看到的是同一个标志。`IconPixmap` 老实返回空数组：实测宿主不读 pixmap，把像素改成纯洋红后截图与改动前逐像素相同。

这里有一个容易忽略的前提：PNG/SVG 写进 `~/.local/share/icons/hicolor` 之后必须重建 `icon-theme.cache`，否则宿主按名字查不到新图标，托盘位会退回宿主自己的兜底图形。`hosts::apply_icons` 在内容有变化时调用 `gtk-update-icon-cache -f -t -q`，安装脚本做同样的处理。

左键发 `Activate`，执行的是 `quicklook --pick`，和 `niri` 里的快捷键走同一条路；右键菜单是「设置… / 重启 / 退出」：设置 spawn 兄弟进程 `quicklook-settings`，重启先 spawn `quicklookd --replace` 再退出自己（新进程轮询等待旧连接释放总线名），退出直接把 Quit 打进自己的 DBus 方法。watcher 重启时会靠 `NameOwnerChanged` 重新登记，所以换宿主、重启面板都不必重启 broker。没有 StatusNotifierWatcher 时不报错退出，只在启动日志里留一行说明。

## Nemo

Nemo 是 Mint 文件管理器。启动器仅对该 Nemo 进程设置 LD_PRELOAD，不修改全局 preload。安装器/运行库校验已验证二进制指纹，清单见 integration/nemo-host.json。已验证 Nemo 6.6.3 / x86_64；升级后拒绝载入，需重核布局、重跑隔离测试，CLI/右键仍可用。

已运行且未载入适配的 Nemo 需要先从菜单退出，复制任务和打开的目录由用户处理。调试方法只在 QUICKLOOK_ADAPTER_TEST=1 时启用，生产接口只有健康检查。

## 插件 v1

扫描 /usr/share/quicklook/plugins/*.toml 和 ~/.config/quicklook/plugins/*.toml，同 ID 用户配置覆盖系统。扩展名优先于精确 MIME，再优先通配 MIME；同类用 priority 调整。重启服务生效。

~~~toml
api = 1
id = "example-custom-text"
extensions = ["custom"]
mime_types = ["application/x-custom"]
priority = 0
command = ["/usr/bin/cat", "{path}"]
output = "text"
timeout_ms = 1000
enabled = true
~~~

{path}/{uri}/{output}/{page} 必须各占一个参数。命令不经 shell，特殊文件名按字面传入。text/html 读 UTF-8 stdout（最多 4MiB），image/pdf 写 {output}（默认最多 128MiB）。失败/超时回退内置预览。

插件以用户身份运行，只安装可信插件。超时 50ms–30 秒，默认 5 秒；取消终止帮助进程组，窗口进程退出时转换子进程终止。当前为进程外结果协议，不提供任意 Rust/GTK 动态库载入接口。

## 2026-10-25 安装与宿主检查更新

本轮细节见 [REVIEW.md](REVIEW.md)。运行时启动器迁移到用户 XDG 数据目录，支持 deb 的只读安装前缀；新配置默认不接管按键，设置保存与接入/注入按钮分开。修复 Caja 菜单回调、地址栏焦点、原生服务恢复、配置原子保存和托管标记校验；新增 ReloadConfig 同步常驻代理的 Nemo 接管状态。打包与迁移见 [PACKAGING.md](PACKAGING.md)。

## 2026-10-26 空格事件修复

400 ms Toggle 冷却和 120 ms HoldLatch 计时均已移除。适配器在按下/释放间锁存，重新获得焦点立即复位；预览窗口只对自己收到的完整 Space 按下/释放关闭，避免移交焦点时吞掉下一次点击或被重复按下反复关闭。详见 FILE_MANAGERS.md。Dolphin 服务菜单改用 Toggle，为新版宿主绑定空格后的开/关语义做准备；23.08 不增加虚假的按键入口。
