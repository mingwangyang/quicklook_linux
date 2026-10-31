# Rust / Wayland 可行性与 P0、P1 实施方案

分析日期：2026-10-25。依据：仓库需求、本机只读检查、对应版本的上游源码及官方文档。

**状态：本文记录安装前的技术分析；当前环境搭建进度见 [安装记录](SETUP_LOG.md)。尚未实现预览功能或进行性能实测。** 本文的方案与验收调整是建议，不代表原需求已实现或用户已同意降低要求。

## 1. 结论与交付边界

推荐 **Rust + GTK4 + Cargo**，原生运行于 Wayland。P0/P1 所列预览类别及统一交互有可用实现路径；难点主要在焦点、文件管理器接入、大文件资源控制和性能验收，不在 Rust 能否调用这些后端。

但不能承诺在任意 Wayland 桌面、任意文件管理器中，无适配地完整复制 macOS 的交互。核心独立于桌面和文件管理器，增强能力通过适配层提供。niri + Nemo 是首发验收组合，Noctalia 不承担文件选择或渲染职责。

本轮产品范围包含原需求全部 P0、P1，包括多选网格、PDF 文本选择、压缩包内单层预览、设计文件、字体、HTML、音频可视化及 Nemo 菜单。Office、epub、ics/vcf、嵌套压缩包、PSD 图层侧栏、其他文件管理器适配按 P2 排除。原 M4 混入 Office，应从本轮移出。（后续：Office 与嵌套压缩包已在 P2 阶段补齐，见 IMPLEMENTATION.md 与 VALIDATION.md。）

必须先处理的条件：

1. 不抢焦点时，键盘事件仍交给原窗口；Esc、方向键等需要来源应用转发，或用户主动进入预览交互模式。
2. Nemo 自带空格预览只传一个 URI；完整多选、选择变化同步需要额外适配。
3. 图片/PDF 仍需 CPU 解码或光栅化；GPU 用于显示合成，不能把“全流程禁止 CPU”作为验收条件。
4. 热启动、缓存切换与首次内容解码须分别计时，复杂格式不能统一保证 80ms/30ms。
5. `.ai`、PSD、RAW、RAR 等有格式子集和文件内容方面的边界，必须用真实样本验证。

## 2. Wayland 窗口与焦点方案

### 首选普通 Wayland 窗口，niri 提供浮动规则

首发优先 GTK4 的 xdg-toplevel，以稳定 app-id 匹配 niri 的 `open-floating true` 和 `open-focused false`。本机 niri 26.04 支持这些设置。这样更适合拖动、固定、全屏和常规应用菜单；应用不主动请求激活。[niri 26.04 窗口规则](https://raw.githubusercontent.com/niri-wm/niri/v26.04/docs/wiki/Configuration%3A-Window-Rules.md)

打开时不抢焦点与之后允许用户点击取得焦点是两个状态。点击预览后，预览自身处理空格、Esc、方向键、Enter、F、P、R、I、T、E、W 等按键。若保持 Nemo 焦点，必须由 Nemo 适配层转发选择变化和关闭事件，不能依靠预览程序监听其他窗口的键盘。

严格保留原 P0 体验时，Nemo 事件转发属于必须完成的工作；若适配不可用，“点击后再操作”只能算明确的降级，不能宣称完整满足无焦点交互。

### layer-shell 作为可选增强

本机实际公布 layer-shell v5，可以使用 GTK4 Layer Shell。但 `KeyboardMode::None` 不接收键盘，`OnDemand` 的聚焦方式由合成器决定；它不解决无焦点收键问题。layer-shell 的定位与移动方式也不同于普通窗口，不宜为了“不抢焦点”就作为唯一窗口后端。[键盘模式](https://wmww.github.io/gtk4-layer-shell/gtk4-layer-shell-GTK4-Layer-Shell.html)

上游明确不支持 GNOME Wayland，因此核心必须保留普通窗口路径。跨桌面基础预览可用，浮动、居中、不激活等增强行为需按桌面能力协商。[支持的桌面](https://github.com/wmww/gtk4-layer-shell)

窗口默认按目标显示器的逻辑工作区居中，宽、高分别限制在工作区 70% 内；显式全屏模式例外。按 GTK 的缩放与显示器信息处理混合 DPI，不使用固定物理像素。精确贴近其他应用中的文件图标，需要适配层提供位置，独立 Wayland 应用不能自行取得。

固定 P 定义为冻结当前预览会话，不被后续选择变化替换；显式关闭仍有效。固定不等同于跨工作区置顶。

## 3. Nemo 接入：已验证事实与未验证部分

已核对本机 Nemo 6.6.3 对应源码：

- 自带预览目标为总线名/接口 `org.nemo.Preview`，对象 `/org/nemo/Preview`，调用 `ShowFile(s, i, b)` 和 `Close()`。Rust 服务可增加兼容入口，X11 的窗口 ID 参数在原生 Wayland 中不能作为跨应用窗口句柄使用。[预览调用源码](https://raw.githubusercontent.com/linuxmint/nemo/6.6.3/src/nemo-previewer.c)
- `nemo_view_preview_files()` 仅取选择列表首项传入，且不传文件管理器排序、过滤或图标位置上下文。该入口不能独自实现空格多选网格或持续导航同步。[文件选择调用源码](https://raw.githubusercontent.com/linuxmint/nemo/6.6.3/src/nemo-view.c)
- 本机动作布局编辑器支持动作快捷键；键位保存在 `~/.config/nemo/actions-tree.json`，并非在 `.nemo_action` 中随意增加 `Shortcut=` 字段。动作通过 `%U` / `%F` 可传多选；原生空格已被视图处理，能否覆盖必须实测。[布局编辑器源码](https://raw.githubusercontent.com/linuxmint/nemo/6.6.3/action-layout-editor/nemo_action_layout_editor.py)

实施顺序：先验证兼容入口的单文件空格 toggle，再验证动作菜单和多选快捷键；最后验证保留焦点时的选择同步、Esc 关闭及排序一致性。安装时要检查 `nemo-preview` 是否占用兼容总线名或存在同名激活文件，不能让两个服务竞争。

严格空格多选和无焦点联动优先研究 Rust 编写的 Nemo 适配扩展；公开扩展接口能否提供足够事件尚未验证，不能假定成立。若必须修改 Nemo 本体，则涉及少量 C 补丁和版本维护；这是坚持完整原版体验时的可能成本，不能冒充纯 Rust 独立应用已解决。

CLI 场景使用本项目确定的目录排序；有适配层时以文件管理器提供的有序 URI 列表为准。不能默认两者排序、隐藏文件策略相同。

## 4. Rust 架构建议

- `quicklook`：轻量 CLI，接受一个或多个路径/URI，以及显式的关闭、导航等命令。管道输入提供 NUL 分隔模式，正确处理带换行的文件名。
- `quicklookd`：会话总线单实例，负责请求、会话状态、路由、调度、缓存与 GTK 主循环；后端按需初始化。优先复用 GIO/GDBus，避免无必要地叠加另一套异步运行时。
- 预览模块：探测 MIME/文件头、准备内容模型、在 GTK 主线程创建视图。耗时 I/O、解码、目录扫描离开主线程；库对象遵守各自线程约束。
- 调度器：当前请求最高优先级，邻近 ±3 仅准备缩略图/首屏；快速导航取消过期任务，以请求代次防止旧结果覆盖新文件。
- 有界缓存：键包含路径、大小、修改时间等；CPU 图像、GPU 纹理、在途任务分别计量。关闭窗口清理内容缓存，媒体和 WebKit 资源设置释放策略。
- 高开销后端：HTML、转换器及部分解析工作可放可回收进程；空闲内存要统计整个进程树，不能只看主进程掩盖成本。

公开接口保留 `org.quicklook.Preview` 的 `Preview(paths)`、`Next()`、`Prev()`、`Close()`，另加明确的 `Toggle`、`Show`、`SetSelection` 语义。`Preview` 的幂等显示与空格 toggle 不应混淆。固定、网格、媒体暂停和目录/压缩包进入均属于同一会话状态机。

先使用同一构建内的 Rust trait 和 MIME/扩展名注册表。原设计把任意 Rust 动态库直接返回 GTK widget，存在 ABI 与 GTK 线程约束，不宜作为外部插件 v1。保留系统及用户插件发现目录，外部扩展建议以版本化 manifest + helper 进程返回受约束的图片、文本、PDF、元数据模型；用户配置可覆盖系统配置。第三方任意 widget 插件生态不属于本轮。

Cargo 负责 Rust 构建、依赖锁定及测试；Meson/Ninja 用于确有需要的原生依赖构建和安装集成。最终程序可安装到现有 `~/.niri-stack/prefix`，会话 DBus 激活文件放在用户总线可发现的标准目录并引用绝对程序路径。

## 5. P0 实施矩阵

| 范围 | 实现路径 | 关键验收与边界 |
|---|---|---|
| 图片 | Rust 图像解码 + GTK/GDK texture；SVG 走 librsvg；动画独立帧调度 | JPG/PNG/BMP/ICO/SVG、GIF/WebP 动画；EXIF/方向/色彩；缩放、平移、旋转、暂停 |
| 超大图 | 优先缩小解码、按需 tile，必要时引入 libvips | 不为所有编码承诺真正渐进解码；避免完整 100MP RGBA 同时驻留 CPU/GPU |
| 音视频 | libmpv Render API 嵌入 GtkGLArea | MP4/MKV/WebM/MOV 与需求音频样本；进度、音量、倍速、逐帧、循环；硬解实际状态可诊断 |
| PDF | Poppler-GLib 渲染 + 自建分页/文本交互 | 连续/单页、页码、跳页、缩略图、文本区域命中与复制；扫描 PDF 无文本层时明确提示 |
| 文本/代码 | GtkSourceView 5 + chardetng / encoding_rs | UTF-8/GBK/GB18030 检测和手动切换；行号、换行；大文件首屏/增量加载，不一次灌入整个 buffer |
| Markdown | pulldown-cmark 等解析；原生内容视图或受约束的 HTML 视图 | 渲染/源码切换；链接、图片、代码块；WebKit 若用于 P0 Markdown，环境依赖相应前置 |
| 目录 | 异步分批枚举，首屏摘要后逐步统计 | 项数、大小、近期修改和内部浏览；递归大小可取消，处理权限与符号链接循环 |
| 压缩包 | libarchive 列表与按需单项解包 | ZIP/TAR/7z/RAR 真实样本；只为选中条目创建受控临时文件，限制解压大小/路径；加密、分卷、solid 变体单独验收 |
| 二进制 | 有界文件头 hex / 字符串 / 类型探测 | 不扫描全文件；文本探测失败可稳定降级 |
| 统一交互 | GTK 事件控制器、手势与会话状态机 | 原 P0 14 项逐项核对；多选网格懒加载；Enter/打开方式使用 GIO 应用关联 |
| 文件复制 | GDK 剪贴板多 MIME provider | `text/uri-list` 与 Nemo 使用的文件复制格式；Ctrl+C 复制文件，文本选择时复制文本并提供显式“复制文件”动作 |
| 集成 | 公共 DBus + Nemo 兼容入口/适配 + niri 快捷入口 | CLI 多选、Nemo 空格/菜单、fzf NUL 路径选择；不在桌面全局劫持 Space |

libmpv 推荐使用 Render API；GTK GLArea 有自身 GL 上下文和 framebuffer，可将结果接入 GTK 场景。应在 render 回调中使用实际 framebuffer，不套用 X11 `wid` 子窗口方案。硬解取决于 GPU、编码和驱动，失败时允许软件解码并保留预览能力。[libmpv 嵌入说明](https://github.com/mpv-player/mpv-examples/blob/master/libmpv/README.md)、[GtkGLArea](https://docs.gtk.org/gtk4/class.GLArea.html)

Poppler 提供渲染与文本信息，并不直接交付完整 GTK4 阅读器；PDF 选择、高亮、跨页和缩放后的坐标映射需要自行实现。GtkSourceView 同样不自动解决超大文本虚拟化。二者是 P0 中实际开发量较大的部分。

“关键帧 seek”偏向快速近似定位，“精确 seek”需要额外解码，不能都叫无损定位。拖动时快速定位、松手后精确定位，分别验收。

压缩列表可以流式读取，但部分格式仍需扫描或解压前序数据，不能承诺随机访问始终瞬间完成。[libarchive](https://www.libarchive.org/)

## 6. P1 实施矩阵

| 范围 | 实现路径 | 关键验收与边界 |
|---|---|---|
| TIFF/HEIC/HEIF/AVIF/JXL | TIFF 解码器、libheif、libjxl，按实测决定 libvips 聚合 | 本机发行版库与实际 codec 插件均需验证；不是安装 Rust image crate 就自动覆盖 |
| RAW | LibRaw 优先读取内嵌缩略/预览图，失败后后台解码 | CR2/NEF/ARW/DNG；预览图缺失或非 JPEG 时有后续路径，不假设所有 RAW 均含 JPEG |
| AI | 检查内容及 PDF 兼容数据，能读才转给 PDF 模块 | `.ai` 扩展名和版本号都不足以保证可直接显示；无有效兼容内容时提供明确降级 |
| PSD | 优先合成图像，不重渲染图层 | `psd` crate 先做样本验证；必要时实现跳过图层区的受限快路径；CMYK/高位深/压缩变体另验 |
| EPS | Ghostscript 转 PDF，转换结果缓存 | 超时、失败提示；缓存键含源文件状态与转换选项；此缓存是原需求允许的专项例外 |
| HTML | GTK4 对应 WebKitGTK 6.0，按需创建 | 默认限制脚本/远端资源与跨目录访问，链接交给明确的打开动作；WebKit 子进程计入内存 |
| JSON/XML/YAML | 有界解析 + GTK 树形视图 | 可折叠、展开、查看原文；限制深度、节点数和实体/别名扩张 |
| RTF | 单独的解析/转换路径与只读富文本视图 | RTF 是文档格式，不能直接套 JSON 树；需覆盖中文、字体样式和控制字，复杂对象失败时保留文本 |
| 字体 | FreeType/fontconfig/Pango，进程内加载 | 字形面板、示例文本、字体信息；不把预览字体安装进系统 |
| 音频可视化 | 从解码的音频采样生成波形/FFT | 封面、波形/频谱，后台限速，不因预览先完整扫描长音频 |
| 不兼容媒体 | libmpv 错误分类 + ffprobe 元数据 | 缺 codec/损坏/不支持时显示元数据及原因；本轮不做自动全文件转码 |
| Nemo 菜单 | `.nemo_action`/适配层调用公共 CLI | 多选列表、打开/关闭、来源上下文一致 |

Adobe 明确第三方查看依赖保存时的 PDF 兼容选项，原需求“现代 AI 都是 PDF、零成本”应改为条件成立时复用。[Adobe AI 格式说明](https://www.adobe.com/creativecloud/file-types/image/vector/ai-file.html)

`psd::Psd::from_bytes()` 源码会解析 layer/mask 区，因此直接使用该 API 不能等同于“只读合成图、完全跳过图层”。libvips 或 FFmpeg 也不能未经样本验证就宣称能兜住全部 PSD。PSD P1 以合成结果为目标，不做图层树或编辑功能。[psd 源码](https://docs.rs/psd/latest/src/psd/lib.rs.html)

## 7. 性能指标：建议拆分验收

下列是待验证目标，不是根据语言选择就能保证的结果。首发在本机 AMD Renoir + niri 上，以 release 构建、本地 SSD、固定样本与显示配置测量；记录 P50/P95、样本大小、缓存状态、帧率和内存峰值。

| 原指标 | 可执行的验收建议 |
|---|---|
| 热启动 <80ms | 分别记录请求到窗口首帧、请求到可辨识内容首帧；在 daemon/基础视图已暖且常见内容命中时以 P95 <80ms 为目标，不能用空壳代替内容 |
| 切换 <30ms | 针对预取命中的图片/文本首屏/PDF 页测试；未命中与复杂格式单列耗时，界面不阻塞且过期请求可取消 |
| 冷启动 <400ms | 单独测试基础图片/文本；首次初始化 WebKit、RAW、EPS 等单列，不作全格式统一保证 |
| 常驻 <60MB | 关闭预览且缓存回收后的整个进程树计量；区分首次启动空闲与用过各后端后的空闲，记录 RSS/PSS；是否达标需实测 |
| LRU 200MB | 作为内容缓存上限，CPU/GPU 分项统计；不代表总内存上限，还需限制在途解码及后端内部缓存 |
| 万行文本 <150ms | 针对约定大小/语言/编码样本测可读首屏，不把全文件高亮完成当作首屏前置条件 |
| 千图连按不落帧 | 有限并发、丢弃过期任务；按显示器刷新率统计超时帧及输入到显示延迟，不为每个中间输入强制解码 |
| GPU 直出、无 CPU 软渲染 | 改为 GPU 合成/缩放优先、视频硬解优先；允许 CPU 解码与 PDF 光栅化，并提供硬解/渲染状态诊断 |

100MP 的一份 RGBA8 图像约 400MB，若同时保留 CPU 副本和 GPU 纹理，远超 200MB 缓存预算。必须以目标显示尺寸、分块和解码限额设计，不能靠 LRU 事后清理解决。

常驻保持热响应与空闲十分钟自动退出也有取舍；退出后的首次唤起按冷启动统计。WebKit 后端可能使整个进程树超过 60MB，需要在原型阶段验证回收策略；不能通过只统计 broker 来宣布达标。

## 8. 本机环境盘点与安装方案

只读检查结果：

| 项目 | 当前状态 |
|---|---|
| 系统/会话 | Linux Mint 22.3 Zena，Ubuntu Noble 基础，x86_64，Wayland |
| 桌面 | niri 26.04，Noctalia 启动配置已存在，Nemo 6.6.3 |
| GPU | AMD Renoir / Radeon Vega；Mesa VA 驱动已装，尚未验证实际硬解 |
| 工具 | C/C++ 编译器、pkg-config、Meson、Ninja 已有；Rust/rustup/Cargo 未发现 |
| 图形开发库 | GTK4 4.14.5、GLib/GIO 2.80、Wayland/EGL/epoxy、librsvg、字体相关开发库已有 |
| 其他已有 | FFmpeg/ffprobe、Ghostscript、fzf、wl-copy、wayland-info；libjxl 开发库 0.7 |
| P0 主要缺项 | GtkSourceView 5、Poppler-GLib、libmpv、libarchive 的开发库 |
| P1 主要缺项 | WebKitGTK 6.0、libheif、LibRaw；libvips 视大图/格式原型结果决定 |
| 可选 layer-shell | 当前 apt 元数据未提供 `libgtk4-layer-shell-dev` 候选；若选用，需固定上游版本源码构建 |
| 安装前缀 | `~/.niri-stack/prefix` 已存在；当前 GTK4 来自 `/usr` |

系统 apt 的 Rust 候选为 1.75，低于当前 gtk4-rs 文档列出的最低 1.83。建议通过 rustup 安装 stable + rustfmt + clippy，实际验证后固定工具链版本，提交 Cargo.lock。GTK4 API feature 上限先按本机 4.14 设置，不能误启用 4.16+；gtk4/glib/gio/sourceview5/webkit6 各绑定必须选兼容的依赖代际。[gtk4-rs 文档](https://docs.rs/gtk4/latest/gtk4/)

下一阶段按以下顺序搭建，本文不执行安装：

1. 安装 Rust 工具链，核对 PATH、版本和 crate 下载；不替换现有桌面栈。
2. 补 P0 库：`libgtksourceview-5-dev libpoppler-glib-dev libmpv-dev libarchive-dev`；核对现有 GTK/EGL/epoxy/Wayland 开发包。
3. 补 P1 库：`libwebkitgtk-6.0-dev libheif-dev libraw-dev`，复用已有 libjxl/TIFF/fontconfig/FreeType/lcms2/Ghostscript；按需增加 `libvips-dev`、HEIF codec 插件和诊断工具 `vainfo`。
4. 只有选择 layer-shell 后端时才源码构建该库，安装进既有 prefix；只给项目设置所需的查找/运行路径，不全局覆盖系统库路径。若原型需要 Nemo 扩展，再补 `libnemo-extension-dev`。
5. 建立 Cargo workspace 与固定工具链，先通过 GTK 窗口、DBus 激活、PDF 首屏、libmpv GLArea/硬解、中文编码的最小验证，再进入完整功能开发。
6. 最后安装用户级 DBus/桌面文件、Nemo 动作和 niri 规则，并验证会话环境传递；配置修改可回滚。

Markdown 若选 WebKit 渲染，第三步中的 WebKit 必须提前到 P0。RTF 转换、PSD 兜底等原型若引入额外工具，单独明确依赖后再安装，不预先堆入整套 Office/Qt 栈。

## 9. 里程碑与完成定义

| 阶段 | 交付与闸门 |
|---|---|
| M0 环境与风险原型 | Rust/GTK4 构建；niri 不激活浮窗；Nemo 空格/多选/事件转发方案；GLArea + libmpv；PDF 文本选择；大文件与空闲内存初测。先验证高风险假设 |
| M1 基础预览 | daemon、DBus、CLI、图片/文本/PDF 首屏、toggle、niri 文件选择入口，测热/冷首帧 |
| M2 完整交互与媒体 | 导航/预取/取消、缩放/平移/旋转、媒体控制、网格、全屏/固定/复制/打开方式/属性 |
| M3 P0 收口 | 编码/Markdown/目录/压缩包/二进制、注册表和发现约定、Nemo 集成；逐项满足原 P0，降级项不能标全绿 |
| M4 P1 收口 | 扩展图片/RAW/AI/PSD/EPS/HTML/结构化数据/RTF/字体/音频可视化及不兼容降级；不含 Office |

完成以功能样本和交互实测为依据：覆盖空文件、损坏文件、中文与特殊文件名、权限失败、大目录、GB18030 四字节字符、动图、多页 PDF、不同媒体 codec、设计文件变体、加密/solid 压缩包及混合 DPI。没有样本通过的格式只能标记为未验证。

M0 若发现无法通过公开 Nemo 接口实现严格无焦点联动，应把“维护最小 Nemo 补丁”与“调整交互验收”列成明确决策，再开展依赖该结论的集成工作。常规 Rust/P0 环境搭建和核心预览不依赖此决策，可以先推进。
