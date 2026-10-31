# P0 / P1 验证记录

日期：2026-10-25，Asia/Shanghai。平台：Linux Mint 22.3 / x86_64、niri 26.04、原生 Wayland。Rust 1.99.1；发布构建使用固定 Cargo.lock。

## 已执行

- 工作区格式检查、Clippy（警告视为错误）通过；测试数随轮次增加，当前为 82 项（core 53、viewer 15、thunar 9、broker 2、nemo 2、clipboard 1）。
- 50 个实际文件样本通过：P0 图片/动画/视频/音频/PDF/文本/Markdown/目录/压缩包/二进制；P1 TIFF/HEIC/AVIF/JXL、DNG/CR2/NEF/ARW、PDF 兼容 AI、PSD 合成图、EPS、HTML/RTF/JSON/XML/YAML、字体。HEIF 与 HEIC 共用后端；未穷举每种编码变体或相机型号。
- 音视频检查实际时长、播放状态和错误；MP4/MKV/MOV 的 H.264 与 WebM 的 VP9 在本机报告 VA-API 硬解。六类音频成功加载波形滤镜。
- 小 GIF/WebP、长 GIF 和 270 帧 WebP 测试；动画帧计数持续增加，K 暂停后停止，恢复后继续。
- 108MP PNG 首轮显示有界预览，1:1 时按原始坐标补解码约 940×486 的可见区域，输出边长不超过 2048；EXIF Orientation=6 的 JPEG 显示尺寸从 320×200 正确转换为 200×320。
- PDF 跳页、缩略图状态保留、连续/单页切换；压缩包按需预览、路径穿越和链接拒绝；网格、导航、固定、开关、关闭清缓存通过。
- 损坏 PNG/PDF/PSD/WebP 显示错误并能继续预览；命名管道和远程 URI 拒绝；包含换行、中文、shell 符号的文件名及 NUL 多选按字面传递；非 UTF-8 文件名可预览且状态正常序列化。
- 快速交替请求大图/PDF/文本 30 次，旧结果未覆盖最终选择；1000 张图片目录识别完整，80 次连续 Next 请求约 0.6–0.7 秒 完成。此测试不是合成器帧率测量。
- 33MiB 静态 WebP 保持原始像素模式，没有误走动画流式路径。
- 插件参数按字面传递；超时终止帮助进程；子进程退出但后代仍持有 stdout 的情况也受超时限制。
- Nemo 6.6.3 隔离会话检查真实 GTK 文件视图选择、单选/多选、Space/Esc、选择同步与固定。调用的是已连接的按键处理逻辑，未伪装为对用户当前窗口的物理键盘验收。
- 已安装程序与 release 构建逐一校验哈希；实际从用户 DBus 服务启动，确认 GdkWaylandDisplay、niri 浮窗且不抢焦点、无缺失动态库或 Wine 依赖。
- 原生窗口截图检查图片、Markdown、结构树、PDF、VA-API 视频、网格、属性侧栏与连续滚动。修正 WebKit 过早报就绪、隐藏属性栏占宽、PDF 侧栏重置、输入焦点判断、暂停按钮不同步等问题。

## 性能

最新一轮发布构建测量：

| 项目 | 观察值 |
|---|---:|
| 预览进程首次启动，请求至准备就绪 | 340.88ms |
| 3 秒回收期内关闭重开，中位 / 最大 | 18.81 / 31.25ms |
| 已缓存文件切换，中位 / 最大 | 17.71 / 50.00ms |
| 切换的数据/控件准备部分，中位 / 最大 | 1.12 / 26.11ms |
| 万行文本首段准备（256KiB），含命令往返 | 34.28ms |
| 关闭回收后的整个进程树 RSS | 6664KiB，约 6.5MiB |

12 次重开、20 次缓存切换，计时采用单调时钟；往返包含 CLI 启动、DBus 和状态轮询。更早冷启动样本为 290.8ms 和 441.11ms，说明机器负载会影响结果。108MP PNG 首轮约 1.4–2 秒，不能与普通小图一样承诺瞬开。

需求中的小于 80ms/30ms 是合成器可见体验目标，**当前没有完成实际首帧及持续键盘浏览掉帧测量，不能声称所有性能硬指标已验收**。缓存切换端到端最大值也超过 30ms。关闭 3 秒后回收图形进程，是快速重开与常驻内存目标之间的明确取舍；超过回收时间后重开会重新初始化窗口。

## 本机证据

证据留在忽略的 .local/，不把相机样本、媒体文件或机器日志推送到仓库：

- all-tests.log、clippy.log、release-build.log。
- smoke-release.log、smoke/results.json、smoke/*.png、smoke/large-image-result.json。
- robustness-result.json、nemo-adapter-result.json、benchmark.json。
- installed-check.json：原生二进制、已安装版本、DBus 激活、窗口规则检查。
- dev-environment/local-install-*.json：实际安装路径、配置和备份。

复现入口见 DEVELOPMENT.md。基础烟测可独立运行；extra-fixtures.py --download-samples 扩展样本。上游测试素材来自 [libheif](https://github.com/strukturag/libheif/tree/v1.17.6/examples)、[libarchive](https://github.com/libarchive/libarchive/tree/v3.7.2/libarchive/test)、[rawpy](https://github.com/letmaik/rawpy/tree/401a9e5e4cd2348b8d11bdddd567915de07c0a83/test) 和 [f-spot/raw-samples](https://github.com/f-spot/raw-samples/tree/54425e2cd53ec17d5fd88ee3277d03f54bf1fe84)。只下载测试文件，不安装这些项目的 Python 包。

## 使用前的限制

- 当前运行的“文件”应用未被重启。Nemo 空格联动需用户先从菜单退出，再从“文件（Quick Look）”启动；CLI、右键预览已安装。用户现有窗口和复制任务没有被自动关闭。
- 文件复制协议依据 Nemo 原生剪贴板目标实现，未覆盖其他文件管理器；没有为测试覆盖用户现有剪贴板。
- PDF 文本选择、鼠标缩放/拖动、拖动进度等控件已实现；没有将程序化检查当成所有输入设备的完整人工验收。
- 颜色、字幕/多音轨、相机新型号、大型复杂 PSD、加密/损坏文档仍依赖系统后端能力；错误会保留默认应用打开入口。
- 图层树、epub、ics/vcf 与其他文件管理器适配（Thunar、PCManFM）按约定保留在 P2。Office 与嵌套包已在本轮补上，见文末两节。

## 四种宿主适配扩展验证

新增 FILE_MANAGERS.md 记录本轮范围。当时工作区 14 项 Rust 测试通过，另通过：

- 独立原生选择窗口打开、重复调用、保持超过回收期限、取消。
- SetSelection 不唤醒已关闭窗口；Nautilus 关闭后的晚到选择事件不重开。
- Dolphin 服务菜单的多 URI 真实命令派发；不将此测试等同于自动点击宿主菜单。
- Nautilus 脚本的中文/换行 URI 多选，Previewer2 三/四参数、Visible 属性、固定、关闭、远端 URI 拒绝。
- Yazi 26.9.1 真实进程、PTY Ctrl+Space/Esc、光标跟随、固定，安装重复执行、既有行内配置和快捷键冲突保护。
- 既有 50 个格式样本与 Nemo 原生适配回归。当前无可用绘制帧，烟测使用 --no-screenshots。

记录：.local/file-manager-tests/results.json、integration-tests.log、integration-clippy.log、integration-release.log、integration-smoke.log、integration-nemo.log。本机没有安装 Nautilus，故其宿主 UI 尚未实测；Dolphin 23.08 不提供该版本不存在的原生动作快捷键。

## 窗口分离与统一设置（本轮）

新增 `crates/core/src/keys.rs`（统一预览键）、`hosts.rs`（按开关落地/撤销各宿主集成）、`crates/daemon/src/settings.rs`（`quicklook-settings` 图形设置）。

- 工作区 26 项单元测试通过，clippy 0 警告；`cargo fmt` 无差异。
- Ctrl+D 分离：`quicklook` 打开图片 → `gdbus call … Action detach` → 新进程 `quicklook-viewer --detached --initial-uri=…` 存活，主窗口 `visible:false` 并按时回收。
- `niri msg windows` 同时列出两个窗口：`org.quicklook.Preview` 为 `Is floating: yes`（弹窗不抢焦点），`org.quicklook.Preview.Detached` 为 `Is floating: no`（正常平铺）。
- 分离窗口存在时再按空格，新弹窗正常显示另一个文件，两个进程互不影响。
- `quicklook --settings` 打开设置窗口（`org.quicklook.Preview.Settings`，niri 规则改为浮动居中），截图确认四组控件与三个按钮均已绘制。
- `quicklook --apply-hosts` 首次写入 1 个文件、移除 1 个文件（移除的是默认空格下不再需要的 `nautilus/scripts-accels` 行），重复执行输出“已是最新，无需改动”，确认幂等。
- 宿主开关实测：写 `host_nemo=false` / `host_yazi=false` 后 `--apply-hosts` 输出“写入 2 个文件，移除 2 个文件”，Nemo 右键动作与 Yazi 插件被删除，`keymap.toml` 的托管块被剥离且用户自带条目保留；删除配置再执行恢复到四个宿主全开（写入 4 个文件）。
- 落地文件内容核对：Dolphin 服务菜单 `Exec` 指向安装路径；Yazi 插件与 `init.lua` 写入同一 CLI 路径；`keymap.toml` 含 `<Space>` 与 `<Esc>` 托管块；Nautilus 的 Previewer2 `.service` 指向 `quicklook-nautilus-bridge`；Nemo 右键动作指向安装路径的 `quicklook`。

仍属未覆盖：分离窗口的平铺行为只在 niri 26.04 上按窗口属性确认，未在其他合成器复核；设置窗口只做了渲染与开关落地验证，没有逐项人工点击全部控件；Dolphin 23.08 依旧只能右键。

## Office 预览与包内浏览（本轮）

新增 `crates/daemon/src/office.rs`（OOXML 只读解析 + 语义化 HTML），并在 `crates/daemon/src/native.rs` 增加 `archive_members()` 一次遍历读取包内 XML。

- 工作区 42 项测试通过（core 26、viewer 15、其他 1），clippy 0 警告，`cargo fmt --check` 无差异。
- 夹具由 LibreOffice 24 headless 从 flat ODF 真实转换而来（不是手写 XML）：`doc.docx` 含标题、正文、制表符、两级标题、项目符号列表和 3×2 表格；`sheet.xlsx` 含「汇总」「明细」两个工作表与数值/文本/空单元格；`deck.pptx` 含 3 张幻灯片。
- docx：`status.body == "office"`，截图确认标题层级、项目符号和两列表格（指标/数值、收入/1200、成本/800）均正确渲染。
- xlsx：两个工作表按原顺序出现，列标 A/B/C 与行号可见，空单元格保留为空白格，数值右对齐前的原文（`420.5`、`19`）未被格式化改写。
- pptx：三张卡片按放映顺序排列，标题与要点分离正确。
- 损坏文件：把文本文件改名成 `bad.docx`，状态返回 `.docx 应当是一个 ZIP 容器；读取包内 XML 失败，文件可能已损坏，或者只是改了扩展名`，窗口不崩溃，仍可用默认应用打开。
- 大表格：2000 行 × 5 列的工作表按 400 行上限截断，界面无卡顿。
- 包内浏览：`outer.zip` 展开后进入 `inner.tar` 再进入 `hello.txt`，`status.nested_path` 依次为 `["outer.zip"]` → `["outer.zip","inner.tar"]` → `["outer.zip","inner.tar","hello.txt"]`，`body` 依次为 archive/archive/text。
- 返回栈：两次 `Back` 逐层退回，第三次在栈空时提示「已经在最外层」；`Escape` 与 `Backspace` 行为一致。
- 层数上限：六层套娃在第 5 次展开时返回 `嵌套压缩包最多展开 4 层，请逐层返回后再继续`，`nested_depth` 停在 4。
- Office 与包内浏览叠加：`bundle.zip` 内的 `notes.docx` 直接按 Office 渲染，标题为 `bundle.zip › notes.docx`，状态行同时显示返回提示。
- MIME 规范化：本机装有 WPS，`docx` 的 content type 被系统别名成 `application/wps-office.docx`；预览改为对 Office 家族使用规范 MIME，标题栏则用「Word 文档 / Excel 表格 / PowerPoint 演示文稿」。

仍属未覆盖：只验证 LibreOffice 生成的 OOXML，没有覆盖 Microsoft Office 原生导出的复杂文档；图表、图片、SmartArt、批注、修订、页眉页脚、脚注均不渲染；xlsx 日期单元格按序列号显示；PDF/Office 的排版与实际打印效果无关；包内浏览只在 zip/tar 上实测，7z/rar 的嵌套未单独验证。

### 静态图内置解码兜底

libvips 8.15 已不再自带 BMP 读取器，BMP 这类格式要靠 `vips-magick` 之类的模块；模块缺失时预览会直接报错。现在 `Kind::Image` 在 libvips 失败后会退回 `image` crate 自带解码器（png/jpeg/gif/webp/bmp/ico/tiff），并在状态行标注「libvips 缺少该格式的解码器，已用内置解码器」。验证方式：正常环境下 BMP/ICO/AVIF 都由 libvips 处理、状态行无提示；把 `VIPSHOME` 指向不存在的目录可复现模块缺失，此时 BMP/ICO 仍能显示（走兜底），AVIF 没有内置解码器则仍报错。

调试提示：`VIPSHOME` 会改变 libvips 搜索 `vips-modules-8.15` 的前缀。若某次运行发现 heif/magick/jxl 加载器全部消失（`vips_type_find` 找不到 `heifload`/`magickload`），先确认 `VIPSHOME` 没有指向无效路径——本次排查正是被一个错误的 `VIPSHOME=/target` 误导，一度误判成「环境缺少 AVIF 解码器」。


## 托盘图标（本轮）

- `quicklookd` 启动后自行占用 `org.kde.StatusNotifierItem-<pid>-1`，并在 `org.kde.StatusNotifierWatcher` 注册；注册成功后 watcher 的 `RegisteredStatusNotifierItems` 里能看到 `org.kde.StatusNotifierItem-<pid>-1/StatusNotifierItem`。
- 图标用应用自己的彩色图标（蓝底白色文档 + 放大镜，`org.quicklook.Preview`）。宿主是本机 niri + noctalia，面板右侧托盘区实测出现该图标；把 `quicklookd` 停掉后它随即消失，确认就是本条目而不是别的应用的图标。取景核对：在 1920×1080 截图上按品牌蓝 `#2f6fd0` 找像素簇定位到托盘位，放大 5 倍确认是白文档叠白色镜片（`/tmp/cand1.png`）。
- 图标自带蓝色底板，因此不依赖面板前景色：浅色主题（本机 `color-scheme=prefer-light`）下是蓝底白图形，深色主题下同样成立，不需要跟随主题换图。
- **踩到的坑**：一开始一直看不到图标，宿主的托盘位显示的是自己的兜底图形。原因是 `~/.local/share/icons/hicolor/icon-theme.cache` 停在两周前——PNG 写进 XDG 目录后没有重建缓存，宿主按名字查不到新图标。`gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor` 之后立刻正常显示。现在 `hosts::apply_icons` 在写入图标后会自动重建缓存，`scripts/install-local.py` 也一样。
- 顺带排除的两个错误方向：把 `IconPixmap` 像素临时改成纯洋红后截图与改动前逐像素相同（`compare -metric AE` = 0），说明宿主根本不读 pixmap；换用主题里的 `system-search` / `system-search-symbolic` 则能显示，但图标会跟着面板配色走。
- 左键 `Activate` 等价于 `quicklook --pick`：实测弹出「选择要预览的文件」窗口。
- 右键菜单由 `com.canonical.dbusmenu` 提供，`GetLayout` 返回三项（设置… / 分隔线 / 退出）；`Event 1 clicked` 实测弹出「Quick Look 设置」窗口（App ID `org.quicklook.Preview.Settings`）。
- 宿主差异：noctalia **不读** `IconPixmap`——把像素临时换成纯洋红后，托盘条截图与改动前逐像素相同（`compare -metric AE` = 0），宿主只用 `IconName` 按图标主题查文件。所以图标能不能显示，取决于 XDG 图标目录里的文件和缓存是否同步（见上面的坑），而不是 pixmap 的字节序。
- 未覆盖：KDE 面板、GNOME 的 AppIndicator 扩展、Waybar 等其他宿主未实测；`Scroll`、`ContextMenu` 两个方法只是返回空，没有做宿主特定的弹出行为。

## Thunar 集成（本轮）

Thunar 的自定义动作是 `~/.config/Thunar/uca.xml` 里的 `<actions>` 根元素，不能用行式追加，只能合并进根节点内部——写错位置会让 Thunar 整个文件解析失败，连用户自己的动作一起丢。因此这里是唯一一处需要解析宿主文件结构的集成。

已核对的内容：

- 生成的文件通过 XML 解析校验（`xml.etree.ElementTree` 可正常读出 `actions` 根与 2 个 `action`），我们的托管块位于 `</actions>` 之前。
- 本机原有的「在此打开终端」动作完整保留，缩进与内容未被改写。
- 托管标记用 XML 注释（`<!-- BEGIN/END quicklook managed integration -->`），与 shell 的 `#`、Lua 的 `--` 两套标记一起由 `strip_managed` 处理。
- 幂等：连续两次 `--apply-hosts` 第二次报告「已是最新，无需改动」，托管块不会重复。
- 关闭 `host_thunar` 时只摘掉自己的块：若还有用户动作则重写文件，若没有则删除文件（避免 Thunar 里留下空的自定义动作菜单）。
- 文件结构异常（缺少 `</actions>`）时不覆盖，改为在报告里提示手工检查——宁可不生效，也不能弄坏用户配置。
- 单元测试覆盖：创建、保留用户动作、幂等、关闭保留、关闭删除、异常文件不覆盖、XML 注释剥离（core 共 33 项）。

未覆盖：宿主右键菜单项没有做自动点击验证（Thunar 不提供按键注入接口，本次尝试注入输入时桌面被锁屏中断）。菜单项是否出现请在 Thunar 里右键确认；改完 `uca.xml` 需要重新启动 Thunar 才会读取。

## Thunar 空格接管（本轮）

目标：让 Thunar 里的空格和 Nemo 一样唤出预览，而不是用默认应用打开。

实现路径与实测：

- 适配器 `libquicklook_thunar.so` 由 LD_PRELOAD 注入，拦 `g_application_run` 后装 `gtk_key_snooper_install`。安装版启动器（`~/.niri-stack/prefix/bin/quicklook-thunar`）实测加载成功，日志首行为「按键旁路器已安装（id=1），预览键 空格」。
- 选择读取链路在真实 Thunar 上跑通（`dbus-run-session` 起独立实例 + `/tmp/thunar-test` 三个文件）：明细视图 16 列，类型名最后两列是 `ThunarFile`、`gchararray`，按类型名定位到第 14 列；自检选中第一行后读到 `["file:///tmp/thunar-test/a.txt"]`。
- 两个坑：侧边栏书签也是 `GtkTreeView`，按祖先链/深度优先会先撞上它（改为只认 `ThunarDetailsView` / `ThunarIconView` 实例类型名）；`ThunarDetailsView` 在 4.18 是复合容器、本身不是树视图（改为在其子树里找真正的树视图/图标视图）。
- 真实按键确实会进旁路器：调试日志记录过 `按键 keyval=0x20 期望=0x20 state=0x0 需要=0x0`。
- 未覆盖：**由人按下空格后的完整链路（旁路器 → DBus Toggle → 预览窗口）没有自动化的端到端证据**。这台机器上没法可靠地注入输入：`gdk_test_simulate_key` 在 Wayland 后端不会把合成事件送进事件管线；`ydotool key space` 实际发出的是字母 `s`（`keyval=0x73`），`ydotool key 57` / `57:1 57:0` 完全没有送达；`wtype` 未安装。因此这一步留给人工按键确认。
- 只对**从启动器起来的** Thunar 生效：已在运行的实例没有加载适配器，空格仍打开默认应用（启动器检测到旧实例时会打印提示）。这也意味着必须先退出旧 Thunar 再从菜单打开。

## 文件管理器入口复审（本轮）

逐个核对五个宿主后发现，只有 Yazi 一直是通的，其余各有断点。这一轮修的是「入口到底能不能被宿主看见」这一层。

- **Dolphin 完全没有接入**：服务菜单文件缺 `X-KDE-ServiceTypes=KonqPopupMenu/Plugin`。本机 `/usr/share/kio/servicemenus/konsolerun.desktop` 等系统自带的样例都有这一行，缺了就会被 Dolphin 整体忽略，右键菜单里不会出现。另外 `MimeType` 原本只写了 `application/octet-stream;inode/directory;`，图片、文本、PDF 都不匹配，等于只对「无类型文件」和目录生效。现改为 `all/allfiles;inode/directory;`。修完 `kbuildsycoca5 --noincremental` 刷新缓存，KSycoca 里能按 UTF-16 搜到 `quicklook` 条目（`~/.cache/ksycoca5_zh_*`），说明已被索引。
- **Nemo 只有右键能动**：适配器 `libquicklook_nemo.so` 只由 `prefix/bin/quicklook-nemo` 加载，但没有任何地方覆盖 `nemo.desktop`——从菜单启动走的是系统条目 `Exec=nemo`，适配器从未加载，空格和多选自然都不生效，只剩右键动作。现在 `--apply-hosts` 会写启动器并覆盖 `nemo.desktop`（只替换 Exec 行，保留 `Actions=open-home;open-computer;open-trash;` 与多语言名字）。
- **指纹不再是硬编码**：之前 `QUICKLOOK_NEMO_FINGERPRINT=d1b031c7a1d60e84` 写死在安装器里，还要先比对 `integration/nemo-host.json` 才肯装库，Nemo 一升级就静默失效。改为 `--apply-hosts` 按当前 `/usr/bin/nemo` 现算 FNV-1a 写进启动器，装库也不再比对 json。
- **Thunar 的 desktop 覆盖原本会丢东西**：之前整份重写成最小条目，系统条目的 `Actions`（主目录 / 计算机 / 回收站）和多语言名字都没了。现在同样改成「只替换 Exec 行」，两个宿主共用 `desktop_with_launcher()`。
- **Nautilus 本机未安装**，集成文件（脚本 + `org.gnome.NautilusPreviewer.service`）照写，但无法实测；已装宿主里只有它没做验证。

实测证据：

- `nemo.desktop` 覆盖后四条 Exec 全部指向启动器：`Exec="/…/prefix/bin/quicklook-nemo" %U`、`… computer:///`、`… trash:///`，`Actions=open-home;open-computer;open-trash;` 保留。
- `thunar.desktop` 同样四条 Exec 指向启动器，`Actions` 保留。
- 启动器内容：`exec env QUICKLOOK_NEMO_FINGERPRINT=d1b031c7a1d60e84 LD_PRELOAD=…/libquicklook_nemo.so … /usr/bin/nemo "$@"`（指纹是本次现算的，与 `/usr/bin/nemo` 一致）。
- 在真实会话总线上用启动器起了 Nemo，`org.quicklook.NemoAdapter.Status` 返回 `Rust Nemo adapter ready; host executable pinned`，stderr 有 `Quick Look: Rust Nemo adapter enabled (pinned host binary).`。
- `--show` 传 `file://` URI 正常（宿主右键菜单传的是 URI，不是路径）：`file:///tmp/thunar-test/b.png` → `kind=image`、`body=image`。
- 多选：`--show a.txt b.png` → `selection_count=2`、`grid=true`。
- 单元测试新增 6 项（desktop 只改 Exec、无匹配返回 None、Nemo 启动器与条目、劫持开关关闭后撤走、缺库时记说明、Dolphin 菜单必需字段），core 共 42 项。

未覆盖：Dolphin 与 Nemo 的右键菜单项没有在界面上点开确认（本机无法可靠注入输入）；Thunar 桌面条目的重启生效仍需人工确认。

### 图标换版（占位）

原来的「蓝底白放大镜」在 22px 托盘尺寸下与「搜索」图标无法区分，用户提出观感不佳。换成蓝底 + 白色文档（带三条灰色文本线）+ 白色镜片，镜片压在文档右下角、外面套一圈品牌蓝描边把它从白底里分离出来。

- 8 个尺寸（16/22/24/32/48/64/128/256）由 `convert -background none … -resize` 从 SVG 重新生成；ImageMagick 不支持渐变（会渲染成黑色），所以全部用纯色填充。
- 图标是编译期 `include_bytes!` 进 `hosts.rs` 的，**改了 SVG 必须重新 `cargo build --release`**，否则 `--apply-hosts` 会因为「内容相同」而跳过写入——第一次就踩了这个坑（装完还是 15:08 的旧文件）。
- 安装后 `icon-theme.cache` 自动重建（704 字节），重启 `quicklookd` 后托盘立即换成新图形。
- 32px 及以下仍能看出「文档 + 镜片」两层结构；16px 会糊成一团，属于占位图的已知限制。

### 右键菜单返回值签名（本轮修）

托盘条目实现了 `com.canonical.dbusmenu`，但右键点了没反应。`GetLayout` 用 `gdbus` 看输出「像是对的」，实际返回的布局节点只有 `(id, 属性表)` 两段，而规范要求 `(id, a{sv}, av)` 三段（第三段是自己的子节点数组）。`gdbus` 会把缺失的尾段直接省略显示，肉眼很难发现；真正的报错在 broker 的 stderr：

```
GLib-GIO-WARNING: Type of return value is incorrect: expected '(u(ia{sv}av))', got '(u((ia{sv}av)av))'
```

宿主按 D-Bus 签名反序列化，类型对不上就把整份布局判为无效，菜单自然出不来。修好后再查：

```
(uint32 1, (0, {'children-display': <'submenu'>, ...}, [<(1, {'label': <'设置…'>}, <@av []>)>, ...
```

每个子节点末尾都带空的 `<@av []>`，且 broker 日志不再有签名警告。教训：**dbusmenu 的每个节点都是三元组**，`av` 的元素类型必须是 `v`（`array_from_iter_with_type(VariantTy::VARIANT, …)`），少一段或元素没包 `v` 都会让宿主静默丢弃。

### 全屏无法退出（本轮修）

现象：预览窗口全屏后再按 `F` 退不回去。

根因是**合成器侧全屏对 GTK 不可见**。用 niri 自己的 `Mod+F`（`niri msg action fullscreen-window`）复现：窗口确实铺满 1920×1080，但 `gtk_window_is_fullscreen()` 仍返回 `false`，GTK 完全不知道这回事。旧代码拿它当判断依据，于是又调一次 `window.fullscreen()`——界面毫无变化，看起来就是「按了没反应」。

修法：`Controller` 自己记一份 `fullscreen: Cell<bool>`，用 `connect_notify_local("fullscreen")` 跟随 GTK 状态，`toggle_fullscreen()` 取 `自己记的值 || window.is_fullscreen()` 作为判据；另外新增 `leave_fullscreen()`，**全屏时 `Esc` 先退出全屏**，再按一次才回上一层/关闭（和其他预览器一致）。`Status` 里也加了 `fullscreen` 字段便于脚本核对。

实测三种路径都正常：应用全屏 `F→F`（1920×1080 → 940×650 浮动）、合成器全屏后按一次 `F`（退出）、`Status.fullscreen` 与窗口实际状态一致。

## 托盘常驻与菜单动作（本轮修）

用户报告「右键菜单里什么都没有，也退不掉」。分层排查后确认是两个独立问题：

- **菜单签名**：见上一节，节点从两段修正为 `(ia{sv}av)`。
- **代理会空闲退出**：`quicklookd` 原先按 `idle_timeout_secs` 自杀，托盘条目随之消失；即便菜单签名正确，用户过一会儿再右键也没有宿主可问。现在 `tray.is_registered()` 为真时跳过空闲退出，托盘没注册时才保留旧行为。
- 安装器默认写 `~/.config/autostart/org.quicklook.Preview.desktop`，登录后即有托盘；`--no-autostart` 可关闭。

实测：

- 总线名 `org.kde.StatusNotifierItem-<pid>-1` 出现在 watcher 的 `RegisteredStatusNotifierItems` 中。
- `GetLayout` 返回根节点 `(ia{sv}av)`，children 为 3 个 `(ia{sv}av)`（设置… / 分隔线 / 退出）。
- `Event 1 clicked` 启动 `quicklook-settings`（窗口 app-id `org.quicklook.Preview.Settings`）；`Event 3 clicked` 让 broker 退出、托盘条目消失。
- 左键 `Activate` 等价于 `quicklook --pick`，弹出文件选择窗口。
- 托盘注册后等待超过空闲阈值，broker 仍存活。

未覆盖：noctalia 面板上的目视与鼠标点击仍需用户最终确认；KDE/Waybar 等其他宿主未测。

## 按住空格只切换一次（本轮修）

用户最早报告的「按一下空格，窗口出现后立刻退出」有两类成因，除了菜单/生命周期问题，还有一类是**按键自动重复**：GDK 对按住不放的键持续补发 `KEY_PRESS`，适配器每次都会 Toggle，窗口刚显示就被下一次重复关掉。

- Nemo：线程内 `HELD_KEY` 记录已处理的键，重复 `KEY_PRESS` 直接返回；收到对应 `KEY_RELEASE` 才清除。
- Thunar：key snooper 同时收到 8（press）和 9（release），按事件类型分类，并用 `PRESSED` 状态挡住重复。
- 两处都通过 `cargo fmt` / `clippy -D warnings` / 单元测试；真机「按住空格」仍列入人工验收项（Wayland 无法注入按键）。

## PDF 缩略图跟随当前页（本轮修）

50 页 PDF 用 `--page 34` 跳转后，侧栏仍停在第 1–2 页，且高亮固定在第 1 页。

- 高亮：`SingleSelection` 在重建时没有设置选中项。现在构造时 `set_selected(当前页)`。
- 滚动：`ListView::scroll_to()` 在列表尚未 map/分配时是空操作。现在改为 `connect_map` 后用 60 ms 定时器再调用，并检查 `is_mapped()`。
- 实测截图 `.local/smoke/pdf-thumbs-long.png`：页码 34/50，侧栏滚到第 33–34 页，第 34 页带橙色选中框；2 页 PDF 翻到第 2 页同样跟随。

## 安装器事务化与卸载器（本轮）

- 安装器补上了此前遗漏的 `--apply-hosts` 调用（放在二进制校验之后，失败不会碰到用户原有集成）。
- 回滚顺序：原先没有 CLI → `--remove-hosts`；有旧 CLI → 先回滚二进制，再用旧 CLI `--apply-hosts` 恢复配套集成。避免只回滚二进制、留下新版本的宿主文件。
- `--remove-hosts` 与 `--apply-hosts --dry-run` 已实现并实测：dry-run 只打印不改文件，remove-hosts 后 Dolphin 菜单/Yazi 插件/Nautilus 服务文件消失，`uca.xml` / `keymap.toml` 只剥托管块。
- `scripts/uninstall-local.py` 已编写并逐行复核：拒绝 `/` 与家目录、`--dry-run`、`--purge`、先 `--quit` 再按 `/proc/<pid>/exe` 校验后 SIGTERM、niri 配置改坏时保留原文件。**尚未做一次真实卸载/重装演练**，这一步留到打包阶段。

## 本轮全量回归（2026-10-25）

- `cargo fmt --check`、`cargo clippy -D warnings` 通过；`cargo test --workspace` **71 项全绿**。
- 原生 Wayland（`env -u GDK_BACKEND -u VIPSHOME`，`--status` 报 `GdkWaylandDisplay`）跑 `scripts/smoke-test.py --profile release`：50+ 夹具全部 PASS，包含 AVIF/HEIC/BMP/ICO/JXL、108MP 原像素补解码、RAR/7z/嵌套包、Office 三件套、PDF 翻页/连续/缩略图、多选、固定、开关、关闭清缓存。
- 环境变量提醒：自动化 shell 里的 `VIPSHOME=/target` 会让 libvips 模块加载失败（AVIF 报“不是已知格式”），`GDK_BACKEND=x11` 会把测试拉到 X11；两者都不是用户会话的真实环境，测试命令要显式 `env -u`。

## 文件管理器空格回归（2026-10-25 修复）

用户反馈「配合界面文件浏览器都不能正常使用」。逐项定位后，Nemo 的注入方案从根上不成立：

- Nemo 6.6.3 的列表/图标视图自己处理 `GDK_KEY_space`，然后调用 `org.nemo.Preview.ShowFile(uri, xid, close_if_already_visible=true)`（源码 `/tmp/nemo/src/nemo-list-view.c:1518`、`nemo-view.c:1210`、`nemo-previewer.c`）。该处理器返回 `TRUE`，事件不再冒泡到窗口级 `key-press-event`，所以 LD_PRELOAD 适配器收不到空格；而旧启动器在已有 Nemo 实例时也只是把窗口抬到前面。
- 新方案：`--apply-hosts` 写 `~/.local/share/dbus-1/services/org.nemo.Preview.service`（`Exec=<prefix>/bin/quicklookd --nemo-compat`），broker 在 `host_nemo && hijack_native` 时占用该总线名并实现 `ShowFile`/`Close`。同时删除旧的 `quicklook-nemo` 启动器与 `nemo.desktop` 覆盖（只删带 `quicklook-nemo` 标记的文件，用户自己的同名条目不动）。
- 实测（原生 Wayland）：
  - broker 停止 → `GetNameOwner org.nemo.Preview` 无主 → 调用 `ShowFile` 后 D-Bus 自动拉起 `quicklookd --nemo-compat`，预览就绪，`status.backend=GdkWaylandDisplay`。
  - `ShowFile` 同一文件第二次 → 预览关闭；重新 `ShowFile` → 打开；`Close` → 关闭。
  - 预览窗口 `niri msg windows` 为 `Is floating: yes`。
  - 以 `/usr/bin/nemo` 正常启动（无 LD_PRELOAD、无 desktop 覆盖）后同样走该总线，不依赖启动方式。
- 代价：Nemo 原生接口只传第一个选中文件，空格预览不再显示多选网格；右键动作 `%U` 仍然支持多选。

Thunar 没有原生预览接口，保留注入，但补上旧实例处理：

- 启动器检查 `/proc/<pid>/maps` 里映射的 inode 是否等于当前 `libquicklook_thunar.so`。只 grep 路径不够：重装会原子替换库，旧进程里留下的是 `…/libquicklook_thunar.so (deleted)`，路径仍然匹配，会被误判为已适配。0.0.5 起改为三态判断（见下条），旧 inode 只记日志、不打扰用户。
- 实测旧实例：`running=2204169 current_inode=3585180 adapted=false`，而被映射的是旧 inode 3585147 且带 `(deleted)`，确认会触发提示。
- 仍需人工验收：接受重启后按空格是否出预览。用户当前运行的 Thunar（pid 2204169）是修复前启动的，必须重启一次；新启动器已装好，从应用菜单再开 Thunar 就会弹出重启提示。

窗口浮动的实际问题是**文件选择器**：`quicklook --pick` 的 GTK4 `FileDialog` 由 `xdg-desktop-portal-gtk` 提供，app-id 不是 `org.quicklook.Preview`，所以旧的 niri 规则匹配不到，选择器被平铺。新增按 `app-id="^xdg-desktop-portal-gtk$" title="^选择要预览的文件$"` 的 `open-floating true` 规则后，实测 `Is floating: yes`，且默认获得焦点。普通预览窗口一直是悬浮的（`Is floating: yes`）。

## 预览窗口抢焦点（本轮修）

排除文件选择器后，预览窗口本身虽然 `Is floating: yes`，但 `niri msg focused-window` 仍显示焦点在 Thunar。根因是 `show()` 只调用 `set_visible(true)`，Wayland 下新窗口不会自动获得焦点；niri 规则又写了 `open-focused false`。于是 Esc/Space 都送回了文件管理器，下一次空格再次 Toggle，表现为「窗口出现后立刻退出」。

- `ui/mod.rs` 的 `show()` 改为 `window.present()`；`integration/niri.kdl` 的预览窗口规则改为 `open-focused true`（独立窗口仍不抢焦点）。
- 修复后实测：`gdbus … Toggle ['file://…/json-ready.png']` → `niri msg focused-window` 返回该预览窗口，`Is floating: yes`，PID 2997705。
- 同一次安装后回归 Nemo 原生路径：`org.nemo.Preview.ShowFile(file://…/12-document.pdf, 0, true)` → 预览获得焦点且悬浮，`Close` 正常关闭。
- 全量门禁：`cargo fmt` / `clippy -D warnings` 通过，`cargo test --workspace` 71 项全绿，release 工作区构建通过；重装后 `smoke-test.py --profile release --no-screenshots` 全量夹具（含 108MP、Office、嵌套包、PDF 交互）全部 PASS。
- 另验证：`quicklook --pick` 选择器仍为 `xdg-desktop-portal-gtk` 且 `Is floating: yes`、聚焦；`quicklook <文件>` 与 `quicklook --show a b`（多选网格）都浮动并获得焦点，再执行一次正常关闭。

## Dolphin 缓存与 Nemo 首次空格（本轮修，2026-10-25）

用户反馈「dolphin、File 两个文件管理器使用有问题」。复测后确认是两个独立问题，均已修复。

### Dolphin：菜单文件已写，KSycoca 缓存没刷新

- 现象：`~/.local/share/kio/servicemenus/quicklook.desktop` 内容正确，但右键菜单里没有「用 Quick Look 预览」。
- 根因：Dolphin 不实时扫描服务菜单目录，菜单来自 KSycoca 缓存。`hosts.rs` 只写文件，没有重建缓存；用户测试发生在 17:06，而缓存直到 17:08 才因手工命令重建。
- 修复：`hosts::apply_at_with` 在 Dolphin 开关启用、或本次撤销了已有菜单文件时，若不在预演模式且数据目录等于真实 `XDG_DATA_HOME`，调用 `kbuildsycoca5 --noincremental`（缺失时依次回落 `kbuildsycoca6`、`kbuildsycoca`）。测试/预演使用临时数据目录，不会触发，避免碰用户缓存。
- 实测：重装触发缓存 mtime `17:08:55 → 17:17:26 → 17:26:20`；Xephyr 隔离会话里启动全新 Dolphin，右键菜单出现「用 Quick Look 预览」并成功打开 PDF（截图 `/tmp/ql-dolphin-menu-new.png`）。
- 限制不变：Dolphin 23.08 没有空格预览接口，空格是宿主自己的打开/重命名行为，只能右键。

### Nemo：第一次空格撞上 D-Bus 激活竞态

- 现象：全新会话或刚重启 broker 后，第一次按空格没有任何反应；第二次起正常。
- 根因：Nemo 的 `ShowFile` 调用本身会触发 `org.nemo.Preview` 激活。旧代码先 `RequestName` 再 `register_object`，名字一到手，排队中的调用立即派发，此时对象尚未注册，D-Bus 返回 `org.freedesktop.DBus.Error.UnknownMethod: Object does not exist at path /org/nemo/Preview`，Nemo 静默忽略。
- 修复：`crates/cli/src/service.rs` 先注册 `NEMO_PATH` 对象，再 `request_name`；抢名失败时丢弃注册。
- 实测：修复前 `dbus-monitor` 精确捕获到上述 `UnknownMethod`；修复后在全新私有总线上连续 5 次以 `ShowFile` 冷启动 broker，5/5 出现 `12-document.pdf — Quick Look` 窗口；Nemo 隔离会话按空格同样出现预览窗口（`xwininfo` 可见 `12-document.pdf — Quick Look`，940x630）。
- 同轮回归真实 Wayland 会话：`ShowFile(..., 0, true)` → 预览窗口 `is_focused: true`、`is_floating: true`，`Close` 正常关闭。
- 限制不变：Nemo 原生接口只传第一个选中文件，空格不显示多选网格；右键动作 `%U` 仍支持多选。

## 预览键可更换：Nemo 改键接管（本轮，2026-10-25）

用户要求「空格呼出预览快捷键可更换」。Thunar 与 Yazi 本来就跟随 `preview_key`，Nautilus 只有 Ctrl/Alt 组合可写进脚本加速键，Dolphin 23.08 没有空格入口；缺口是 **Nemo 把 Space 写死在视图里**，改配置后仍然只认空格。

根因与修法：

- 窗口级 `key-press-event` 钩子看不到 Space——Nemo 视图先消费并返回 `TRUE`。但 GTK 的 `gtk_key_snooper_install` 在事件分发给焦点控件之前运行，能先看到按键。
- `crates/nemo-adapter` 增加 snooper：命中配置键时读取当前 `NemoView` 选择并走原有 `Toggle`；`preview_key != Space` 时把裸空格返回 `TRUE` 吞掉，避免 Nemo 自带预览再弹一次；重复按键仍由 `HELD_KEY` 去重。
- `hosts::apply_nemo` 改为双模式：默认空格只写原生 `org.nemo.Preview.service`；非默认键生成 `bin/quicklook-nemo` 启动器（`LD_PRELOAD` + 动态计算的宿主指纹）并覆盖 `nemo.desktop`。切回空格或关开关时删除这两项、恢复原生 service。
- 启动器沿用 Thunar 的 inode 校验：`/proc/<pid>/maps` 里的 `.so` inode 必须等于当前文件，否则询问重启，避免重装后旧进程仍映射 `(deleted)`。

实测（Xephyr :99 + `dbus-run-session` + 私有 XDG 目录，真实 Nemo 6.6.3，配置 `preview_key="Ctrl+Space"`）：

- 适配器日志：`Nemo 按键旁路器已安装（id=2）`；`Ctrl+Space` 记录 `key=0x20 state=0x4 预览键=Ctrl+空格 handled=true`；裸空格记录 `拦截宿主空格（预览键 Ctrl+空格）`。
- 视图选择读取到测试目录里的两个文件；`Ctrl+Space` 打开窗口（`ctrl_space_preview=6291460`），裸空格不产生新窗口（`plain_space_preview=none`），再按 `Ctrl+Space` 得到同一个窗口 id（`second_ctrl_space_preview=6291460`），确认切换而不是叠加。
- 清理：测试结束后杀掉私有 Xephyr 与测试进程，执行真实 `quicklook --apply-hosts` 撤掉测试启动器，恢复默认空格集成；未触碰用户正在运行的 Nemo/Thunar/Dolphin/Yazi。

未覆盖：真机上从应用菜单重启 Nemo 后的按键需要用户手动确认；从终端直接执行 `/usr/bin/nemo` 不经过启动器，仍是原生空格行为（文档已注明）。


## 浅色/深色主题与托盘重启（2026-10-25）

### 主题

新增 `config.theme`（`light` / `dark`，默认 `light`），设置窗口增「外观」区段。落地分三处：

- GTK：`gtk-application-prefer-dark-theme` 跟随配置；默认浅色时显式设为 false，系统是深色也不会让预览窗口变深。
- 预览窗口：`style.css` 为浅色基础样式，深色时再叠加 `style-dark.css`，避免维护两份完整布局。
- 文档内容：`web.css` 改用 CSS 变量，根节点按主题加 `dark` 类；Office 表格/幻灯片的内联样式也改用同一组变量。SourceView 在浅色下用 `Adwaita`、深色下用 `Adwaita-dark`。

Xephyr `:99` + 私有 XDG 配置实测，截图保存在 `.local/theme-tests/`：

- `ql-theme-light.png` / `ql-theme-dark.png`：同一 HTML（标题、链接、代码块、表格、引用）分别为浅底深字与深底浅字，页面与标题栏配色一致。
- `ql-theme-text-light.png`：Rust 源码在浅色主题下使用 `Adwaita` 配色，行号、关键字、字符串可读。
- `ql-theme-settings-light.png` / `ql-theme-settings-dark.png`：设置窗口正确显示「外观 → 主题：浅色/深色」，整个窗口随配置切换。
- 测试显示为独立 Xephyr，窗口被移到隐藏工作区；结束后已关闭并清理临时配置。

未覆盖：主题切换后，已经打开的预览窗口不会热重载，需要重新打开；文档注明这一点。

### 托盘“重启”

dbusmenu 改为「设置… / 重启 / 退出」，节点 id 依次为 1/2/3/4（3 是分隔线）。重启流程：

- 旧 broker 先 spawn `quicklookd --replace`，再执行原有 `Quit`。
- 新进程注册对象后轮询 `RequestName`（50ms 间隔，最多 10 秒），等旧连接释放 `org.quicklook.Preview` / `org.nemo.Preview` 后接管；不依赖手动 kill 或 D-Bus 激活。
- 隔离会话验证：旧 `service_pid=1161291` 发起 `Event 2 clicked` 后退出，`owner_before=:1.0` 变为 `owner_after=:1.3`，新进程带 `--replace` 运行。
- 真实会话验证：`before=1223175 after=1258514`，新 PID 存活且 `quicklook --status` 正常；`GetLayout` 返回四个节点（设置… / 重启 / 分隔线 / 退出）。
- 重启会关闭正在显示的预览，文件管理器窗口不受影响。

全量门禁：`cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 82 项全部通过；release 构建与安装后的二进制/两个适配器哈希一致。


## 默认布局设置与空格重复（2026-10-25）

### 为什么「默认浮动」变成了「默认平铺」

真机排查（niri 26.04）：

- `quicklook --status` 显示 `backend = GdkX11Display`，`niri msg windows` 里预览窗口 `App ID: ""`、`Is floating: no`、`Window size: 1920 x 1080`。XWayland 窗口没有 app-id，`integration/niri.kdl` 的 `app-id` 规则一条都命中不了，于是被平铺。
- 环境变量来源：拉起 broker 的 shell 里有 `GDK_BACKEND=x11`（`/proc/<broker>/environ` 可见），viewer 继承后走 X11 后端。
- 修复：`service.rs::prefer_wayland()` 在有 `WAYLAND_DISPLAY` 时对 viewer 与托盘 spawn `env_remove("GDK_BACKEND")`；`quicklook --settings` 同样处理。
- 修复后同一命令：`backend = GdkWaylandDisplay`、`App ID: "org.quicklook.Preview"`、`Is floating: yes`、`Window size: 940 x 650`。
- 规则文件内容没变但 niri 未自动重载 include 时，`niri msg action load-config-file` 之后新窗口即按规则浮动；因此同步之后固定调用一次重载。

### `window_mode`（浮动 / 平铺）

新增配置与设置区段「窗口 → 默认布局」，默认浮动；未知值退回浮动。规则由 `crates/core/src/niri.rs` 托管，安装脚本不再单独维护第二份逻辑（`integration/niri.kdl` 与 Rust 模板由 `floating_template_matches_installed_integration` 钉住）。

真机端到端（改 `~/.config/quicklook/config.toml` 后重启 broker）：

- `window_mode = "tiled"` → `~/.config/niri/quicklook.kdl` 预览规则改为 `open-floating false`（选择器与设置窗口仍为 true，独立窗口仍为 false），新窗口 `niri msg windows` 显示 `Is floating: no`。
- 还原配置 → 规则改回 `open-floating true`，新窗口 `Is floating: yes`。两次都不需要手动跑 `niri`，broker 启动时完成同步与重载。
- 设置窗口截图 `.local/layout-tests/settings.png`：区段顺序为 预览快捷键 / 窗口 / 外观 / 文件管理器集成，「默认布局」下拉显示「浮动（默认）」；设置窗口本身 `App ID: org.quicklook.Preview.Settings`、`Is floating: yes`。
- 未安装 niri 集成（没有 `quicklook.kdl`）时 `sync` 返回 None，保存提示「安装时未启用 --with-niri，默认布局不会生效」，不写文件也不报错。

### 按一下空格像按了很多次

来源是键盘自动重复，且不止一条路径：Nemo 原生空格在自己的 GTK 里持续补发 `org.nemo.Preview.ShowFile`（适配器层的 `HELD_KEY` 去重管不到这条路），预览窗口内的空格关闭同样会被补发。GTK4 不把「这是重复按键」告诉应用（`gdk` 无 `is_repeat`），因此在 viewer 侧按时间窗收口：`Controller::allow_toggle()` 对 `ShowMode::Toggle` 与空格关闭共 400ms 冷却。

真机验证（`quicklook --show` 打开后并发 5 次 `quicklook README.md` 切换）：

- 打开后 `visible=true`；5 连发（约 300ms 内完成）后 `visible=false`，即只发生一次关闭，其余被丢掉；冷却之后再按一次恢复 `visible=true`。
- 同一 viewer 进程（`viewer_pid` 未变），说明不是进程重启造成的假象。

未覆盖：真机上由 Nemo / Thunar 长按空格的体感仍需用户手动确认（Wayland 输入注入不可用，见陷阱 7）。


## 按文件管理器分别设置快捷键（2026-10-25）

配置模型：`preview_key` 为全局默认，新增 `preview_key_nemo` / `_thunar` / `_yazi` / `_nautilus` 四个覆盖（空字符串=跟随，非法值退回跟随）。读取侧统一走 `Config::nemo_key()` 等；Nemo / Thunar 适配器与 `hosts.rs` 的写入判断都改成各自的键，不再共用全局 `key()`。

单元测试 `hosts::tests::per_host_keys_override_only_their_own_host`：全局 `Ctrl+Space` + Nemo `F3` + Yazi `F6` + Nautilus `Alt+F7` 时，Yazi keymap 只出现 `<F6>`（不含 `<C-Space>`）、`scripts-accels` 为 `<Alt>F7 Quick Look`、Nemo 装上按键旁路器启动器；清空覆盖后三者一起回到跟随全局。`config::tests::host_keys_follow_the_global_default_unless_overridden` 覆盖解析与落盘归一化。

真机验证（安装版 `quicklook --apply-hosts`，用后即还原）：

- 写入 `preview_key_yazi = "F6"`、`preview_key_nautilus = "Alt+F7"` 后：`~/.config/yazi/keymap.toml` 变成 `on = "<F6>"`，`~/.config/nautilus/scripts-accels` 变成 `<Alt>F7 Quick Look`，其余宿主集成未动（`写入 2 个文件`）。
- 追加 `preview_key_nemo = "F3"` 后 `--apply-hosts --dry-run` 预报会写 `~/.niri-stack/prefix/bin/quicklook-nemo` 与 `~/.local/share/applications/nemo.desktop`（预演不落盘）。
- 还原配置再 apply：keymap 回到 `on = "<Space>"`，`scripts-accels` 被移除（全局空格不可绑定，符合 Nautilus 限制），Nemo 启动器不存在。`config.toml` 与集成状态均已复原。
- 设置窗口截图 `.local/layout-tests/settings-host-keys.png`：「预览快捷键」区段为 全局默认 + Nemo / Thunar / Yazi / Nautilus 四行，每行下拉首项「跟随全局默认」，选中它时该行的 Ctrl/Shift/Alt 变灰不可点。

未覆盖：Nemo / Thunar 用独立按键后的真机按键手感仍需用户重启对应文件管理器确认（适配器在进程启动时读一次键；Wayland 输入注入不可用，见陷阱 7）。

## 收尾复验：污染环境下的 broker（2026-10-25）

用 `GDK_BACKEND=x11` 环境里启动的 broker（`/proc/<service_pid>/environ` 确认带着该变量）直接呼出预览：

- `quicklook --status` 仍报 `backend=GdkWaylandDisplay`、`app_id=org.quicklook.Preview`；`niri msg windows` 里 `Is floating: yes`、`Tile size: 940 x 650`。
- 即 viewer 侧的 `prefer_wayland()`（只在有 `WAYLAND_DISPLAY` 时 `env_remove("GDK_BACKEND")`）不依赖 broker 自身干净，覆盖终端/IDE 里手动起服务的场景。
- `quicklook --close` 后 grace 期内 viewer 自行退出，`viewer_running=false`、`visible=false`，桌面上不留任何测试窗口。

## Caja 接入验证（2026-10-25）

单位层面：`core` 66 项测试全绿，其中新增 `keys` 状态机 9 项（从 Thunar 适配器搬来，两处宿主共用一份判定）、`hosts` 的 Caja 3 项与 desktop 备份/还原 1 项——

- `caja_writes_menu_extension_launcher_and_entries`：写出 `~/.local/share/caja-python/extensions/quicklook.py`、`prefix/bin/quicklook-caja` 与五个 `caja*.desktop` 覆盖；启动器里带 `LD_PRELOAD`、`/usr/bin/caja --quit`，且**不含 `pkill`**。
- `disabling_caja_keeps_user_desktop_entries`：关掉 `host_caja` 后启动器与扩展被撤，用户自己写的 `caja-browser.desktop` 原样保留。
- `caja_without_adapter_library_keeps_space_unhijacked`：`.so` 缺失时只留右键扩展、不生成启动器，并在报告里说明原因。

真机层面（安装版 `--apply-hosts`，`caja --no-desktop --no-default-window` 起一次不带窗口的实例）：

- `~/.cache/quicklook/caja-adapter.log`：`按键旁路器已安装（id=1），预览键 空格` —— 符号查找（`caja_view_get_type` / `caja_view_get_selection` / `caja_file_get_uri` / `caja_file_list_free`）与 `gtk_key_snooper_install` 在真实 Caja 1.26.3 进程里都成立。
- 临时在扩展文件末尾加一行 stderr 标记后重启 Caja，标记打印出来且无 traceback —— python3-caja 确实导入了我们的 `Caja.MenuProvider`；随后用 `--apply-hosts` 还原文件。
- `caja --help-all` 里有 `-q, --quit`，因此启动器的重启改成调用它而不是发信号（写第一版时以为 Caja 和 Thunar 一样没有退出接口）。
- desktop 覆盖只改 `Exec=` 首段：与系统条目逐行 diff，除 `Exec` 外完全一致（`caja-browser.desktop` 仍保留 `--no-desktop --browser %U`）。
- `scripts/uninstall-local.py --dry-run`：Caja 扩展、启动器与带 `quicklook-caja` 标记的覆盖条目都在删除清单内，用户自己写的同名条目走「保留」提示。
- 设置窗口截图 `.local/layout-tests/settings-caja-keys.png`：「预览快捷键」区段现在是 全局默认 + Nemo / Thunar / Caja / Yazi / Nautilus 五行，「文件管理器集成」区段多出「Caja（MATE）」勾选项，提示文案同步提到 Caja。
- 顺带发现（未修）：设置窗口默认 620 px 高，内容更长，打开时滚动位置会停在焦点控件附近，「预览快捷键」区段要先往上滚才看得到。区段变多以后更明显，已记入待办。

未覆盖：无——Caja 的完整按键链路已改由 `scripts/test-caja-adapter.py` 在隔离会话里自动点验（见下），不再依赖用户手动确认。

上述真机检查先跑在 debug 库上，`cargo build --release --locked --workspace` 通过后又用 release 安装重跑一次（六个二进制 + 三个 `.so` 与构建产物 sha256 一致，`caja-adapter.log` 仍打印旁路器安装），测试数 87。

## Caja 隔离会话端到端点验（`scripts/test-caja-adapter.py`）

Caja 单实例、且 `caja --select=URI` 在 1.26.3 上不生效，所以不能像 Nautilus 那样用 CLI 直接喂选择。整套改跑在隔离栈里：Xephyr `:99 -screen 1280x800x24 -ac -nocursor` + marco（没有窗口管理器时 Caja 拿不到 focus-in，Ctrl+A 选不中）+ `dbus-run-session`（地址写文件后各进程再读取），临时 HOME/XDG_*，剥掉 `WAYLAND_DISPLAY`/`GDK_BACKEND`，另起一个专属 `quicklookd`；`xdotool` 只在这条 X 通道里用，不碰用户真实的 niri 会话。测毕销毁，`pgrep -u $(id -u) -a caja` 确认无残留。

release 构建实测（debug 亦通过）：

~~~json
{"caja": "MATE caja 1.26.3", "display": ":99", "isolated_bus": true, "snooper_installed": true,
 "space_opens_selection": true, "repeat_presses_open_once": 1,
 "real_hold": {"press_events": 1, "activations": 1}, "recovers_from_lost_key_release": 2,
 "rss_growth_kib": 1056, "space_multi_selection": true, "empty_selection_leaves_space_alone": true,
 "remapped_key_opens": true, "remapped_key_closes": true, "plain_space_inert_after_remap": true,
 "context_menu_provider_queried": ["用 Quick Look 预览"], "menu_command_opens_preview": true,
 "uninjected_space_untouched": true, "no_type_assertions": true}
~~~

- **一次按下只切一次**：`repeat_presses_open_once=1`。顺带量化了一件之前只能推断的事——XTEST 合成的 keydown **不会自动重复**：按住 1.2 s，适配器日志只有 1 条 `kind=8 keyval=0x20`，切换 1 次。所以「长按只开一次」验的是状态机不把一次按下拆成两次；真键盘的重复由 `HoldLatch` 的 120 ms 间隔和 viewer 的 `allow_toggle()` 兜住。
- **抬起丢失能自愈**：按下 → 抬起（抬起可能被预览窗口吃掉）→ 350 ms 后再按下，第二次仍算新按键，锁存按时解除（`recovers_from_lost_key_release=2`，预览最终关闭）。
- **引用计数**：15 轮预览后 Caja 常驻内存只涨 1 056 KiB（阈值 8 MiB）。这条是 `caja_view_get_selection` 返回 transfer-full `GList` 的回归防线——早先用错释放函数（`caja_file_list_unref` 只 unref 不清链表）每次按键漏一条。
- **改键与不吞键**：Caja 预览键改成别的组合键后该键开、该键关、裸空格交回宿主；空选中时旁路器返回 0，空格仍是 Caja 自己的「选中首个文件」。
- **右键**：`MenuProvider` 被查询到 `用 Quick Look 预览`，执行该菜单项真的出预览。
- **注入覆盖**：不带 `LD_PRELOAD` 起的 Caja 不接管空格，旁路器只存在于注入过的进程。
- **无类型误用**：全程 Caja 的 stderr 没有 `GLib-GObject` 关键断言，证明按实例类型名分派成立（1.26.3 的选择列表里装的是 `GFile`/`GLocalFile`，不是 `CajaFile`）。

## viewer 切换冷却的反噬：主动关闭后的空格被吞

`allow_toggle()` 的 400 ms 冷却挡住了连击，也顺带挡住了「Esc 关掉后立刻再按空格开」：时间戳只在切换时写，关闭不写回。`scripts/test-nemo-adapter.py --profile release` 因此在稳定的第 3 步超时——`Key(space)` → `Key(Esc)` → 选两个文件 → `Key(space)`，最后这次 Toggle 距上一次不足 400 ms 被丢掉，网格永远出不来；同一份夹具改用约 1 s 间隔手工重放，`grid / selection_count=2` 正常，说明问题在 viewer 不在适配器。

修法是按「谁关的窗」分流：用户主动结束的关闭（Esc、关窗按钮、`Close` 方法、分离成独立窗口、交给默认应用）走 `Controller::close_by_user()`，先清 `last_toggle` 再关；只有切换自身驱动的关闭（同一选择再按一次、预览窗口内的空格、选择清空）保留时间戳，连击保护不变。改完 `cargo test --workspace` 全绿，Nemo 套件与 Caja 套件（debug/release）随后都过，真机会话的冒烟与健壮性套件也重跑通过。

## 2026-10-25：代码检查、Caja 修复与 deb

本轮结果以本节和 [REVIEW.md](REVIEW.md) 为准；旧章节中的 prefix/bin 启动器、默认开启接管、PATH 同名链接等描述已被当前实现取代。

- 工作区 **93 项 Rust 测试通过**；fmt 干净；clippy 以 `-D warnings` 通过；release 工作区构建通过。
- 真实 Caja 1.26.3 在 Xephyr + marco + 私有会话中通过：单选空格开/关、多选网格、改为 Ctrl+Space、空选不接管、地址栏空格不误触发、菜单 provider 和真实 `MenuItem.activate()` 回调、无注入保留原行为。15 轮开关 RSS 增长 1060 KiB。测试后无残留测试 Caja。
- 修复菜单回调前，旧测试只是执行同一条 CLI，无法发现信号参数数量错误；现已直接触发菜单对象信号。未把这项描述成 Wayland 真机鼠标点击验收。
- 54 个格式夹具、Office 文本结构、嵌套压缩包、PDF、多选、导航、固定和缓存释放通过。此次图形回归使用 `--no-screenshots`，没有独立测量合成器实际出帧。
- 损坏输入、设备/远程 URI、含特殊字符与非 UTF-8 文件名、符号链接提取拒绝、大幅静态 WebP、过期解码取消通过；1000 图片目录中快速切换 80 次耗时 552.44 ms。
- Nemo 原生 Rust 适配回归通过。首次隔离回归发现测试选择器依赖用户列表视图；修改测试为临时 keyfile 中强制列表视图后通过，未改用户布局。
- 原生总线生命周期验证通过：默认配置不持有 org.nemo.Preview；用户启用并应用后获取；撤销时释放，保持当前预览打开。
- deb 用 dpkg-shlibdeps 根据 ELF/发行版符号表计算依赖；包无个人目录路径和桌面接入维护脚本。私有 dpkg 根目录中完成安装/卸载、校验和、只读前缀下接入、重复应用不改动、恢复用户服务/desktop、保留个人配置测试。
- 当前系统 `apt-get -s install` 通过：1 个新增包（quicklook）、0 个移除。没有用 sudo 在宿主执行 deb 安装。
- 源码自装版在临时 HOME/XDG 中完成安装、重复安装和卸载；原 Caja desktop 恢复，偏好保留。本机现有源码版已更新，未自动重启任何文件管理器。

证据：`.local/review-tests.log`、`review-clippy.log`、`review-release.log`、`review-caja.log`、`review-regression.log`、`review-nemo.log`、`review-local-lifecycle.log`、`review-deb-test.log`；包检查结果 `.local/deb-tests/results.json`。这些是本地测试产物，不纳入 Git。

仍有边界：GNOME Files 缺真实宿主界面验收；Thunar 需目标桌面按键验收；跨合成器剪贴板互通和实际首帧性能目标没有在本轮补齐。Nemo 的私有 ABI 改键适配仅允许已验证的 x86_64 构建；公开原生空格入口不受此限制。

## 2026-10-26：去除空格等待与六宿主重验

- 删除全局 400 ms Toggle 冷却和宿主 120 ms 时间锁存；20 次无 sleep 的完整按键状态机测试通过。
- 93 项工作区测试通过；clippy -D warnings / fmt / release 构建通过。
- 真实宿主在 Xephyr + 私有会话中连续五组空格开/关：Nemo 6.6.3、Caja 1.26.3、Thunar 4.18.8、Nautilus 46.4；每种也验证焦点移到预览后，下一个完整空格立即关闭。操作不插入旧的 400 ms 等待。
- Yazi 26.9.1 在真实 PTY 中以裸空格连续五组开/关通过，每组约 50–55 ms。
- GTK 文件管理器每组开/关约 136–235 ms，包含测试驱动、焦点切换和状态查询；不是合成器出帧性能测量。
- Caja 的重复按下、长按、多选、地址栏、改键、菜单和内存回归通过；测试已移除“必须等待 400ms”的旧前提。长按期间只打开一次。若焦点已转到预览，松开它收到的 Space 可关闭一次，不会来回开关。
- Dolphin 23.08.5 实测裸空格不触发 Quick Look。官方 26.04 新增服务菜单快捷键，源码要求 KIO >= 6.24；已核对源码，未在本机升级并运行新版。旧版额外 Qt 适配/回移植仍未实现。
- Nautilus 使用 Ubuntu 46.4 deb 临时解包运行。它的依赖也仅解包到 .local/space-hosts，通过只读文件系统命名空间提供 Tracker 的模块绝对路径，未写系统目录。初始路径栏焦点导致 Ctrl+A 未选文件的问题已在测试驱动中通过明确选中夹具修正；协议桥未发现故障。

当前证据：.local/space-host-final.log、.local/space-key-tests/results.json、.local/space-caja.log、.local/space-tests.log、.local/space-clippy.log、.local/space-release.log。

## 2026-10-26：登录后的 Thunar 启动路径回归

- 现场检查：Quick Look V0.0.1 服务正常，接管设置保留；niri 的 Mod+E 经自定义 launch-file-manager 直接执行 thunar，运行进程没有适配库。问题在启动路径，不能靠重复启动预览服务解决。
- 新增用户级 Thunar 两个专用 D-Bus 激活覆盖与 thunar.service 配置片段；原文件备份/恢复，不改通用文件管理器选择，不自动重启用户宿主。
- 源码 --with-niri 对已知脚本命令加托管块，保留 GDK_BACKEND=wayland 和清除 GTK_IM_MODULE 的行为。真实脚本执行测试验证特殊路径/参数、重复应用、撤销与不存在适配器时的回退；未知脚本不改写。
- 95 项 Rust 测试、fmt、clippy -D warnings、release 构建通过；源码安装/重复安装/卸载能恢复 Caja desktop、niri 启动脚本、Thunar 激活文件，保留用户随后添加的修改。
- 真实 Thunar 4.18.8 在隔离 Xephyr + 私有会话中，通过适配启动器、gtk-launch 菜单入口和 D-Bus 冷启动各验证五组连续空格开/关，以及预览焦点内下一次空格关闭。均未在测试进程环境预置 LD_PRELOAD，启动后核对实际宿主映射中存在适配器；每组约 134–207 ms。
- 私有 D-Bus 的 DISPLAY 在 Xephyr 创建后同步更新，避免激活服务使用父会话的显示。systemd 配置通过真实解析器检查，包含路径中的空格、美元和百分号；没有为此启动用户 thunar.service。
- 本机已更新启动脚本和接入配置，niri validate 通过。用户确认空格预览已经恢复。

证据：.local/reboot-tests.log、reboot-clippy.log、reboot-release.log、reboot-source-test.log、reboot-{launcher,desktop,dbus}-results.json、reboot-install.log。修复包沿用 V0.0.1 应用版本，deb 修订为 0.0.1-1。

## 2026-10-26：Caja 终端普通命令与扩展加载

- 触发条件：终端直接输入 caja，没有经过旧版适配启动器。用户扩展现于启动时加载 Rust 按键库；全新原始进程的环境不含 LD_PRELOAD，仍可在首次右键菜单之前按空格预览。
- 真实 Caja 1.26.3 在隔离显示/私有总线中，经原始命令、菜单和预加载加用户扩展三种方式，各验证五组连续空格开/关与预览内空格关闭。每组约 135–158 ms；实际宿主映射包含适配库。双加载使用进程共享标记，只有一个按键回调。
- 完整回归以扩展方式运行，验证空格、多选、地址栏不误触发、空选不接管、Ctrl+Space 改键、长按单次切换、丢失松开恢复、右键 MenuItem.activate 与关闭接管后原行为。15 轮开关内存增长 812 KiB，无 Caja 类型断言或残留测试进程。
- 原显式预加载方式的同一套完整回归也通过，15 轮开关内存增长 1144 KiB。双加载冷启动另核对实际进程日志，只安装一次按键回调。
- 本机已安装新版用户扩展与 Rust 适配，用户确认终端直接运行 caja 后已恢复。现有 python3-caja 1.26.0 即可，无新增系统安装包；没有替换 /usr/bin/caja 或创建同名 PATH 文件。
- 97 项 Rust 测试、fmt、clippy -D warnings、release 构建通过。原始启动方式需要 python3-caja，仍列为可选宿主依赖；缺少它的系统保留显式启动器方式。
- 源码安装/卸载回归与 deb 私有安装/卸载、只读前缀、用户配置恢复、依赖解析检查通过；包内 Rust 导出入口可加载，在非 Caja 进程返回 0，不初始化 GTK，生成的扩展可编译。

证据：.local/caja-native-tests.log、caja-native-clippy.log、caja-native-release.log、caja-native-{extension,desktop,preload}-results.json、caja-native-full.log、caja-native-fallback.log、caja-native-double-load-results.json、caja-native-install.log、caja-native-source-test.log、caja-native-deb-test.log。当前修复包为 V0.0.1 的 deb 修订 0.0.1-2。

## Markdown 流程图（Mermaid，本轮）

- 实现：`crates/daemon/src/mermaid.rs`；内置 Mermaid 11.17.2（gzip 953 KiB，见 `integration/vendor/README.md`），只对含 ```mermaid 的 Markdown 打开 JavaScript 并注入用户脚本。
- 单元测试 5 项：围栏识别（```/~~~、大小写、信息串多余参数）、反例（rust 块、无信息串、`mermaidx`、闭合围栏长度不匹配）、bootstrap 携带主题与处理器名、bundle 解压后 >1 MB 且为预期 UMD 构建且命中缓存。
- 真机（niri / Wayland，debug 与 release 均验证）：
  - `/tmp/mdtest/flow.md`（flowchart + sequenceDiagram）→ `--status` 的 `mermaid` 为 `ok:2`，截图确认「开始 → 判断 → 是/否 → 执行/跳过」正确绘制。
  - 普通 Markdown（只有 rust 代码块）→ `mermaid` 为空串，确认这条路径仍然没有开启 JavaScript。
  - 深色主题（独立 `XDG_CONFIG_HOME`，不改用户配置）→ 图随 `theme='dark'` 切换，连线标签底色与画布一致，无浅色小方块。
- 冒烟测试新增夹具 `04b-diagram.md` 与断言：含图文档 `mermaid` 以 `ok:` 开头、普通 Markdown `mermaid` 为空；`PASS Markdown Mermaid flowchart rendering (and plain Markdown stays JS-free)`。
- 全量回归：102 项测试、clippy 0 警告、fmt 干净；`scripts/smoke-test.py --profile release` 全绿（含 54 夹具、Office、嵌套包、108MP、PDF 翻页）。
- 未覆盖：Mermaid 全部图型未逐一目视；超大图（数十节点）没有性能测量；CSP 被宿主策略覆盖的极端情况未测。

### deb 0.0.1-3 打包验证

- `python3 scripts/build-deb.py --version 0.0.1-3` → `dist/quicklook_V0.0.1-3_amd64.deb`（3.6 MB，比 0.0.1-2 大 1 MB，差值是内置 Mermaid 资源），SHA-256 `8896a480ca17dada0018184cd94cd6d48c92a4d6aaacbfacea988305c109ba44`。
- 包内 59 个条目、28 个图标文件、6 个可执行文件（quicklook / quicklookd / quicklook-viewer / quicklook-settings / quicklook-nautilus-bridge / quicklook-clipboard）；0.0.1-5 起带 `postinst` / `prerm` 两个维护者脚本（见下文）。
- `python3 scripts/test-deb.py dist/quicklook_V0.0.1-3_amd64.deb` 全项通过：私有 dpkg 安装/卸载、md5sums、只读安装前缀、用户接入幂等、原生服务与 desktop 恢复、无 PATH 同名替换、apt 依赖解析。
- 本轮同时修掉 test-deb.py 的 locale 隐患：它断言 `Setting up quicklook` 这类英文串，在中文 locale 下 dpkg 输出「正在设置 quicklook」会误判失败；现在子进程统一带 `LC_ALL=C`。

### niri 窗口规则改由 `--apply-hosts` 维护（本轮）

现象：deb 安装后预览窗口被 niri 平铺，不是浮在中间。

根因：窗口的 `open-floating` / `open-focused` 全在 `~/.config/niri/quicklook.kdl` 里，而这份文件此前只由源码安装器的 `--with-niri` 生成；`niri::sync_at()` 开头 `if !path.is_file() { return Ok(None); }`，文件不存在时什么也不做。deb 按设计不改家目录，所以从源码版迁移到 deb（或全新装 deb）后规则彻底缺失。

修复：`crates/core/src/niri.rs` 新增 `install_at()` / `remove_at()`，`hosts::apply_at_with()` 在任一宿主启用时调用 `install_at`（任一宿主都不启用时调用 `remove_at`），于是 `quicklook --apply-hosts` 与设置窗口的「应用文件管理器接入」都会创建并维护规则：写 `quicklook.kdl`、给 `config.kdl` 补 `include "quicklook.kdl"`（已有则不重复）、缺 `Ctrl+Alt+P` 时追加选文件绑定，最后跑 `niri validate --config`，不通过则两个文件一起回滚。

验证（本机 niri 26.04 / Wayland，deb 0.0.1-4 实装）：

- `quicklook --apply-hosts` → 「写入 1 个文件」「写入 ~/.config/niri/quicklook.kdl」。
- `~/.config/niri/config.kdl:275` 出现 `include "quicklook.kdl"`；`quicklook.kdl` 尾部有 `binds { Ctrl+Alt+P hotkey-overlay-title="文件预览" { spawn "/usr/bin/quicklook" "--pick"; } }`。
- `niri validate --config ~/.config/niri/config.kdl` → `config is valid`。
- `niri msg action load-config-file` 后 `quicklook --show /tmp/mdtest/flow.md`，`niri msg windows` 报告 `App ID: "org.quicklook.Preview"` / `Is floating: yes`。
- 截图确认窗口浮在桌面中央、圆角裁剪生效（内容为 Mermaid 流程图）。
- 重复执行 `--apply-hosts` 不再写入（幂等）；`--apply-hosts --dry-run` 不落盘（`install_dry_run_writes_nothing` 覆盖）。
- 单元测试新增 5 项（`ensure_include_adds_once`、`strip_include_removes_comment_and_blank_line`、`picker_binding_only_when_absent`、`install_creates_missing_integration_and_remove_cleans_up`、`install_dry_run_writes_nothing`），niri 模块共 8 项；全仓 107 项通过，clippy 0 警告，fmt 无差异。

未覆盖：非 niri 合成器（KDE/GNOME/Sway）不需要这份规则，`install_at` 因找不到 `~/.config/niri/config.kdl` 直接返回 `Ok(None)`，此处只做了代码路径确认，没有在那些会话里实跑。

### 安装时自动接入，以及非 niri 桌面的行为（本轮）

问题一：deb 装完还要用户自己敲 `quicklook --apply-hosts`，否则窗口规则缺失、预览被平铺。这不该由用户记住。

做法：新增 `packaging/debian/postinst` 与 `prerm`（`scripts/build-deb.py` 打进 `DEBIAN/` 并置 0755，依赖里补 `util-linux` 以取得 `runuser`）。两者都只认「发起这次安装的用户」——`PKEXEC_UID`，退回 `SUDO_USER`；识别不到就打印提示直接退出，绝不去猜、也不碰其他用户。以该用户身份执行时清掉 `VIPSHOME`（它会带偏 libvips 的模块搜索路径）：

- `postinst configure`：`config.toml` 里 `hijack_native = true` → `quicklook --apply-hosts`（完整接入，等于升级自愈）；否则 → `quicklook --apply-window-rules`（只写窗口规则，不抢空格、不加菜单项）。新增的 `--apply-window-rules` 就是为此加的窄入口，背后是 `hosts::apply_window_rules_only`。
- `prerm remove|purge`：`quicklook --remove-hosts`。升级（`upgrade`）不执行，避免升级时把接入清空。

验证（deb 0.0.1-5 实装，niri 会话）：

- 先把 `~/.config/niri/quicklook.kdl` 删掉、`config.kdl` 里的 `include` 行也删掉（模拟"规则丢失"），再 `pkexec apt-get install` 覆盖同版本以上：`正在设置 quicklook (0.0.1-5) ...` 期间直接打印 `写入 ~/.config/niri/quicklook.kdl`，装完 `config.kdl:277` 已有 `include "quicklook.kdl"`，`niri validate` 通过。**全程没有手动命令。**
- `--apply-window-rules` 窄入口：`XDG_CONFIG_HOME=/tmp/fakehome/config` 下只有 `niri/config.kdl` 时执行，仅写出 `quicklook.kdl` 一个文件，家目录里没有生成任何宿主接入文件。
- `prerm remove`：以 root + `PKEXEC_UID` 执行，输出「写入 1 个文件，移除 21 个文件」，niri 规则文件删除、`config.kdl` 里 quicklook 引用归零、`Thunar/uca.xml` 剥掉托管块；随后 `quicklook --apply-hosts` 恢复「写入 22 个文件」，`niri validate` 通过。
- 两个脚本 `sh -n` 语法通过；未做非 root 直接执行的兼容性验证（dpkg 始终以 root 调用）。

问题二：非 niri 能不能用。

结论：**程序不依赖 niri**——GTK4 画窗口、D-Bus 传指令，Wayland 与 X11 都能跑；差别只在窗口布局策略。新增 `crates/core/src/compositor.rs` 做识别并给出对应指引，`--apply-hosts` 在找不到 niri 配置时打印它：

- GNOME / KDE / Cinnamon / MATE / Xfce 这类不自动平铺的桌面：新窗口本来就浮动并接收焦点，开箱即用，无需任何规则。
- niri：规则由程序托管（写文件 + `niri validate` + 失败回滚）。
- sway / Hyprland：属于平铺合成器，必须有一条 `for_window` / `windowrulev2` 才会浮动。它们的配置语法差异大、本机没有会话可验证，所以**只识别并打印可直接粘贴的片段，不擅自改配置**，避免写坏别人的桌面配置。

4 项单元测试覆盖识别优先级（niri 配置优先，其次 sway、Hyprland 的配置文件或 `SWAYSOCK` / `HYPRLAND_INSTANCE_SIGNATURE`，都没有则按"无需规则"）、片段内容，以及"普通桌面不返回片段"。

未覆盖：sway / Hyprland / GNOME / KDE 会话里没有实机跑过（本机只有 niri），只验证了识别与文案；平铺桌面上的实际浮动效果需要在那类会话里确认。

### 登录自启选项（本轮）

需求：设置里要能选择登录时自启。

实现：`Config` 新增 `autostart`（默认 `false`），由 `hosts::apply_autostart` 维护 `$XDG_CONFIG_HOME/autostart/org.quicklook.Preview.desktop`，只启动常驻 broker `quicklookd`（通知栏图标 + 预热），不启动预览窗口。设置窗口新增「启动」区段；「保存设置」会立刻对齐该条目（`hosts::apply_autostart_only`），不必再点「应用文件管理器接入」；`--remove-hosts` 与「撤销全部接入」之外的卸载路径也会撤掉它。

为什么默认关闭：这个项目其余集成也默认不接管（`hijack_native = false`）。登录自启属于同一类「改用户会话行为」的动作，不该在安装时悄悄发生；关掉时 broker 仍会在第一次预览时被自动拉起，只是没有通知栏图标。

为什么只启动 broker：预览窗口是重量级的（GTK + WebKit + 解码线程），登录时启动等于白占内存；broker 是轻量 GIO 进程，负责托盘与快速应答。

`Exec` 写绝对路径：`broker_path()` 先找 `<prefix>/bin/quicklookd`，找不到退回「与当前可执行文件同目录」，最后才是裸名字。开发构建里 binary 在 `target/debug/`，两者不同目录，而 autostart 启动环境的 `PATH` 未必可靠，所以不能只写 `quicklookd`。

验证：
- 单测 3 项：`autostart_entry_points_at_the_broker`（断言 `Exec` 指向 `quicklookd` 且不含 `quicklook-viewer`、图标名与 `[Desktop Entry]` 头正确）、`autostart_second_apply_is_a_noop_and_disable_removes_it`（幂等 + 关闭即删除 + 重复删除不再报账）、`autostart_source_escapes_quotes_in_the_path`。
- 假 HOME（`XDG_CONFIG_HOME=/tmp/autohome/config`）实跑：`--apply-hosts` 在 `autostart = true` 时写出条目，`Exec="~/work/code/self/quick_look_linux/target/debug/quicklookd"`（绝对路径）；改成 `false` 后输出「移除 …/autostart/org.quicklook.Preview.desktop」且空的 `autostart` 目录被一并清掉；`--remove-hosts` 同样撤销。
- 设置窗口截图确认「启动」区段的开关、三段说明文字与既有区段排版一致。
- 全仓 114 项测试通过，clippy 0 警告，fmt 干净。

未覆盖：没有真的重新登录一次看 broker 是否被拉起（会中断当前会话）；niri 以 `niri-session` 运行时支持 XDG autostart 是依据其自身说明与机器上既有的 `~/.config/autostart/` 条目（fcitx、Flameshot 等），本机 `~/.config/autostart` 也确实被使用。

### 0.0.1-7 打包

仅修订版号递增，产物内容与 0.0.1-6 一致（登录自启功能已在 0.0.1-6 落地并验证）；重新打包后核对包内 6 个可执行、3 个适配器库、`postinst`/`prerm` 与图标齐全。

### 0.0.1-8 修「迁到 deb 后 Thunar 空格失效」

现象：从源码版迁移到 deb 后，Thunar 里按空格没有预览。

定位：`ps` 看 Thunar 父进程是 `sh -c $HOME/.niri-stack/prefix/bin/launch-file-manager`（用户 niri 的 Mod+E 快捷键），
该脚本直接 `exec ... thunar "$@"`，绕开 desktop 覆盖 → 适配器没有进进程（`/proc/<pid>/environ` 无 `LD_PRELOAD`，
`maps` 无 `libquicklook_thunar.so`）。只有源码安装器的 `--with-niri` 会迁移这条脚本，deb 的 `--apply-hosts` 从不做。

修复：把迁移逻辑搬进 `crates/core/src/hosts.rs`（`apply_launcher_shim`），候选路径为
`<prefix>/bin/launch-file-manager` 与 `~/.niri-stack/prefix/bin/launch-file-manager`，受管标记
`# BEGIN/END quicklook managed file-manager launcher`，仅匹配已知命令，关闭接管时剥块还原。
新增 4 项测试（改写 + 幂等、关闭还原、自定义命令保留 + note、host_thunar 关闭时不动），core 共 118 项。

实测：安装 0.0.1-8 时 postinst 直接打印「写入 ~/.niri-stack/prefix/bin/launch-file-manager」；
脚本内受管块指向 `~/.local/share/quicklook/launchers/quicklook-thunar`；重启 Thunar 后
`/proc/<pid>/environ` 含 `LD_PRELOAD=/usr/lib/quicklook/libquicklook_thunar.so`、`maps` 命中该库 4 处，
适配器日志打印「按键旁路器已安装（id=1），预览键 空格」；`quicklook --status` 端到端
（`--show` 一个 markdown）返回 `visible=true body=markdown`。

注意（踩坑）：用 debug 二进制跑 `--apply-hosts` 时 `install_prefix()` 是 `target/`，
找不到 `lib/quicklook/libquicklook_thunar.so`，会把启动器与 desktop 覆盖判为「未启用」并删除——
测这类改动要用安装版或 `--apply-hosts` 之外的路径，别在用户机器上直接跑 debug 的 apply。

### 0.0.3-1 应用版本 0.0.3，托盘右键加「关于」

- `Cargo.toml` 的 `version` 从 0.0.1 提到 0.0.3，`quicklook --version` 输出 `quicklook V0.0.3`；
  设置窗口标题与状态 JSON 的 `version` 同步（都取 `quicklook_core::VERSION`）。
- 托盘右键菜单新增「关于 Quick Look」（dbusmenu id=5），点开是只读小窗（app-id
  `org.quicklook.Preview.About`），内容为图标、名字、版本 V0.0.3、一句功能说明与许可，关闭即退。
  由 broker 以 `quicklook-settings --about` 拉起——broker 只链 gio/glib，不引入 GTK。
- 私有会话验证菜单结构：`GetLayout` 返回 设置…(1) / 关于 Quick Look(5) / 分隔线(3) / 重启(2) / 退出(4)。
- 单测：托盘布局断言子节点数 5、标签序列含「关于 Quick Look」；全部 118 项通过。
- 关于窗口截图确认渲染（图标 / 「版本 V0.0.3」/ 说明 / 关闭按钮）。

### 0.0.3-2 修「空格关闭有时候失效」

三个独立成因，全部补上：

1. **锁存卡住（最像「按两下才关」）**。适配器用「按下/松开」维护一个「空格还按着」的
   锁存，避免长按的自动重复连续 Toggle。原来只在宿主窗口 `focus-in` 时清锁存；可是
   预览窗口出现后会抢走焦点，空格抬起事件打到预览窗口，宿主收不到 → 锁存永远停在
   「按下」→ 下一次空格被当成自动重复吞掉，用户必须按第二下。
   修法：适配器同时挂 `focus-out-event`（焦点被预览窗口拿走时立刻清），并给锁存加
   400ms 兜底时限（`quicklook_core::keys::STUCK_TIMEOUT`），抬起彻底丢失时也能自愈。
   `HoldLatch` 从 `Cell<bool>` 改为 `Cell<Option<Instant>>`；Nemo 适配器的
   `HELD_KEY` 同样带时间戳。viewer 侧 `show()` 已有的 `space_held.set(false)` 复位保留。

2. **焦点不在文件视图里（最常见的「按空格没反应」）**。Thunar/Caja 适配器原先只从
   「当前焦点控件」向上找文件视图，找不到就放弃；只要点过侧边栏书签、工具栏按钮或
   状态栏，焦点就不在列表/图标视图里，空格被静默忽略。
   修法：焦点不在视图里（且不在输入框/文本框里，避免抢走正在输入的字符）时，退回到
   窗口里第一个文件视图。Caja 新增 `inside_text_input()` 守卫；Thunar 沿用 `walk_marker`。
   Nemo 的空格走原生 `org.nemo.Preview` 协议（`hijack_native` 时由本项目接管），
   不依赖该适配器的视图定位，故未改。

3. **陈旧 `space_held`**。viewer 每次 `show()` 都把 `space_held` 复位，避免上一次会话
   残留的「按下」让新窗口一出现就立刻关掉。

质量门：119 项测试（core 93，含新增 `latch_self_heals_when_the_key_up_never_arrives`）、
clippy 0 警告、fmt 干净。三个适配器与 viewer 均重新构建。

## CSV / TSV 表格预览（本轮）

- 单元测试 10 项：引号/逗号/字段内换行、`""` 转义、CRLF 与 BOM、分号嗅探、制表符识别、短行补齐、数值列右对齐、文本列左对齐、2000 行截断、单元格转义。
- 真机（niri / Wayland，debug 构建）：
  - `sales.csv`（201 行 × 7 列，中文表头、含逗号备注）→ 表格正常，行号与粘性表头到位，数量/单价/金额右对齐。
  - `gbk.csv`（GBK 编码）→ 自动识别为 GBK，中文正常，可按 E 切换编码。
  - `euro.csv`（分号分隔，字段 `"键盘; 黑色"` 含分隔符）→ 嗅探到分号，引号内的分号保持单格。
  - `report.tsv` → 制表符分隔，标题栏显示「TSV 表格」。
  - `ragged.csv`（第二行少一列、第三行多一列）→ 按最长行补 4 列，空单元格留白。
  - `big.csv`（5000 行 × 7 列）→ 渲染 2000 行，底栏「仅显示前 2000 行（共 5000 行）」；请求到内容约 830 ms（含 WebView 初始化），200 行小表约 390 ms。
  - 深色主题（独立 `XDG_CONFIG_HOME`）下表格、粘性表头、斑马纹配色正确。
  - `data.json` → 默认落在「源码」页，「结构」页仍可切换。
- 冒烟测试新增 `04c-table.csv` / `04d-table.tsv` / `04e-semicolon.csv`，断言 `status.body == "csv"`，输出 `PASS CSV/TSV preview renders as a table (comma, tab, semicolon sniffing)`。
- 回归：130 项测试全绿、clippy 0 警告、fmt 干净。
- 未覆盖：带换行的引号字段会让行高变大（`white-space: pre` 保留换行，未做折叠）；欧洲格式数字（`1.234,56`）不识别为数值；超过 4 MiB 的表只解析前 4 MiB，底栏会提示。

## 启动器不再弹「需要重启」对话框（0.0.5）

问题：Thunar 是单实例程序。若当前运行的实例没有加载适配器，再点桌面入口只会把旧窗口抬到前面，空格依然无效；之前的启动器用 zenity 弹「需要重启 Thunar 才能启用，现在重启吗？」，每次覆盖安装（`libquicklook_thunar.so` 的 inode 变了）或从终端起过一次 Thunar 就会再弹一次，很烦。

改法（`crates/core/src/hosts.rs` 的 `launcher_script`，Nemo / Thunar / Caja 三者共用）：

- 用会话总线上的 `GetConnectionUnixProcessID` 拿真正的宿主 pid（不用 `pgrep`——它是全局的，会看到别的会话里的同名进程，导致等待永远不结束）。
- 三态判断：`adapted`（maps 里的 inode 等于当前库）/ `stale`（映射的是旧版库，功能仍在）/ `foreign`（根本没加载）。maps 里的 `…\ (deleted)` 用前缀匹配才认得出来。
- `stale`：只写一行日志，不重启、不打扰（旧版适配器仍在工作）。
- `foreign`：静默 `--quit` → 按会话总线所有者 pid 等它消失（最多 3 s）→ 仍不退才 `SIGTERM`/`SIGKILL` → 再 exec 带 `LD_PRELOAD` 的新进程。全过程只写 `~/.cache/quicklook/launcher.log`。
- 需要旧行为时设 `QUICKLOOK_LAUNCHER_ASK=1`，才会回到 zenity 询问。

实测：
- 真实会话里把 Thunar `--quit` 后用 `/usr/bin/thunar /tmp` 起一个未适配实例（`/proc/<pid>/environ` 无 `libquicklook_thunar`），再执行启动器 → 日志出现「运行中的实例未加载适配器，自动重启」，无弹窗，新实例 pid 的 `LD_PRELOAD` 含 `/usr/lib/quicklook/libquicklook_thunar.so`，窗口正常。
- 覆盖安装后运行中的 Thunar 映射的是旧 inode → 启动器只记「用的是旧版适配器，未重启」，不弹窗。
- 没有实例在跑时启动器直接 exec，不写重启日志。
- `bash -n` 三个生成脚本语法通过；129 项测试、clippy 0 警告、fmt 干净。

## 空格关闭有时候失效（0.0.6）

现象：选中文件按空格打开预览没问题，但想用空格关掉时偶尔要按第二次。

成因有两条，都在「按键抬起事件跑到预览窗口去了」这条主线上：

1. **自动重复判定窗口过大。** 适配器用「锁存是否按下」判断这次按下是不是自动重复。预览窗口抢走焦点后，第一次按下的抬起事件打到预览窗口，适配器手里的锁存只能靠焦点变化或 400ms 超时清除。用户在 400ms 内按第二下（很正常的手速），这次按下被当成自动重复吞掉——表现就是要按第二次。
   改成 `quicklook_core::keys::REPEAT_GAP = 150ms`：只有距上次按下不足 150ms 才算重复（自动重复默认约 33ms），更长就按新的一次按下处理，照常发 `Toggle`。Thunar / Caja 共用 `HoldLatch::rapid()`，Nemo 的 `is_repeat` 同样改用该常量。
2. **冷启动期间的第二次 Toggle 走了「再打开」分支。** 第二次 `Toggle` 在窗口还没真正可见时到达，`show()` 里原来只在「可见且同一选择」时关闭，于是又把同一文件 show 了一遍，窗口留在屏幕上。
   现在 `show()` 额外记录 `shown_at`：同一选择、不可见、且距上次 show 不足 `INFLIGHT_TOGGLE = 1.5s` 时，直接 `close()`。
3. 附带：预览窗口新增 `EventControllerFocus` 的 `leave` 处理，焦点离开时清掉 `space_held`，避免抬起事件丢失后残留 true。

验证（真机 niri / Wayland，`ydotool` 注入按键）：

- 用 Thunar 选中 `sales.csv`，两次空格间隔在 0.15–0.55s 之间随机跑 14 轮，14/14 全部关闭（修复前同区间约 9/12 失败，失败间隔集中在 0.21–0.31s）。
- 直接对 viewer 调 `Toggle`/`Status`：单次 Toggle → `visible=true, ready=true, body=csv`；再 Toggle → 全 false；间隔 0.2s 两次 Toggle（冷启动竞态）→ 最终 false。
- 修好后 `cargo test` 130 项、clippy 0 警告、fmt 干净。

## PDF 连续滚动 / 全屏提示 / 设置分类（0.0.7）

用户要求：PDF 默认连续滚动、放大时所有页一起放大；全屏后知道怎么还原、修好「F 还原有时候失灵」；默认窗口大小可在设置里调；设置菜单要分类；用户要知道设置怎么生效；安装时自动检测并完成注入启用。

- **PDF 默认连续滚动**：`Config.pdf_continuous`（默认 true）。打开 PDF 直接纵向列出全部页面；格式栏按钮在「连续滚动 / 单页模式」之间切换；`Page Up/Down` 回到单页。默认按 `640 * aspect` 的高度请求每页，超出窗口可滚动。
- **所有页一起放大**：`State.pdf_zoom` + `State.pdf_pages`（弱引用集合）。`+ / = / -`、`0`、`1` 与 `Ctrl+滚轮` 统一走 `zoom_all_pages()`，会同时改所有 canvase 的缩放和请求高度。子画布在连续模式下 `set_scroll_zoom(false)` 且滚轮完全让路（`if !scroll_zoom { Proceed }`），否则指针落在某一页上时 Ctrl+滚轮只会缩放那一页（用户实测「只有两页之间才能缩放」即此原因）。
- **全屏提示与 F 还原**：内容区包进 `gtk4::Overlay`，顶部悬浮提示「全屏中 · 按 F 或 Esc 退出全屏」，进入全屏显示、5 秒后自动隐藏，底部状态行同步。原先 `notify::fullscreen` 只要合成器没确认就把标记清回 false，导致下一次 F 又「请求进入」，看起来像 F 失灵；现在只采信 `is_fullscreen()==true` 的方向，并加 600ms 自身切换保护期。
- **默认窗口大小可调**：`Config.window_width / window_height`（默认 940 / 650，`0` 表示按内容自动）。设置窗口「窗口」页可调，`size_window()` 用 `config.window_size()` 取代写死的 940/650。
- **设置窗口分类**：`build()` 改为顶部说明横幅 + `gtk4::Stack` / `StackSidebar`，左侧分类：快捷键 / 窗口 / 外观 / 文件管理器 / 渲染与缓存 / 启动。保存按钮成功后前缀「✔ 已保存并生效。」并显示 `apply()` 返回的生效说明；恢复默认会提示还需点「保存设置」。
- **一键自动检测**：`hosts::detect_hosts()` 遍历 PATH 的可执行文件，`--autodetect-hosts` 写入配置并启用接管；设置窗口「文件管理器」页新增「自动检测并启用」按钮。deb 首次安装（没有 config.toml）时 postinst 直接走 `--autodetect-hosts`。
- **PDF 右键拖动平移**：`Canvas` 增加 `GestureDrag` + `BUTTON_SECONDARY`，按住右键即可抓取平移（不必再 Shift+左键）。

关键坑：GTK 窗口首个可聚焦控件是页码 `SpinButton`，`key()` 一旦判定焦点在 `Editable/SpinButton` 就把所有按键原样放行，于是「刚打开 PDF 时 F、+、- 全部无效」。修法是在 `pdf_view` / `pdf_continuous` 建完后 `canvas.grab_focus()` / `list.grab_focus()`，让键盘一开始就落在画布上。

验证：133 项 Rust 测试、clippy 0、fmt 干净。真机（niri / Wayland）确认：19 页 PDF 默认连续滚动（`pdf_continuous=true`）；`ydotool type '='` 使 `pdf_zoom` 1.0 → 1.2，截图多页同步变大；设置窗口六分类与「默认窗口大小 940×650 + 按内容自动」正常显示；连续模式格式栏显示「连续滚动 · Ctrl+滚轮或 +/- 缩放全部页面 · Page Up/Down 回到单页」。

## PDF 连续滚动模式下 Page Up / Page Down 翻页（0.0.8）

**现象**：连续滚动为默认模式后，Page Up / Page Down 看起来完全没反应。

**根因（两条）**：
1. `change_page()` 走的是 `load()`，而 `load()` 会把 `state.pdf_continuous` 置 `false`，`render()` 又因为 `config.pdf_continuous` 为真立刻调 `toggle_pdf_continuous()` 切回连续模式，同时 `pdf_continuous()` 把 `pdf_zoom` 重置为 `1.0` —— 视觉上等于什么都没发生。
2. 原 match 臂在 `if editing { return Proceed }` 之后，焦点落在页码 `SpinButton` 时 Page 键被原样放行。

**修法**：`key()` 最前面（`editing` 判断之前）直接拦截 `Page_Down | KP_Page_Down` / `Page_Up | KP_Page_Up` 并 `return Propagation::Stop`，交给新函数 `pdf_page_step(delta)`：
- 单页模式：仍 `change_page(delta)`。
- 连续模式：取 `state.pdf_scroll`（`pdf_continuous()` 里 `s.pdf_scroll = Some(scrolled.downgrade())` 记录），按 `step = adj.page_size() * 0.92` 直接滚一屏，`y` 夹在 `[lower, upper-page_size]`；**不改 `s.page`、不重新 load**，因此不会被自动切回逻辑重置；状态栏显示「N% · Page Up/Down 翻页」。
- `load()` / `pdf_continuous()` / `toggle_pdf_continuous()` 三处把 `pdf_scroll` 置 `None`，避免指向已销毁的 `ScrolledWindow`。

**验证**：133 项测试、clippy 0、fmt 干净。真机隔离验证（`dbus-run-session` 私有总线 + 启动前后 window id 差集定位窗口 + 先确认 `niri` 焦点确实在预览窗口）实测 19 页 PDF 的 `vadjustment` 连续推进：1433 → 2172 → 5430 → 9231 → 13032，Page Down 向下、Page Up 向上各一屏。

**重要教训（ydotool）**：`ydotool key N` 里的 `N` 是 **X keycode = evdev + 8**，不是 evdev 键码。传 109（KEY_PAGEDOWN）实际发的是 `'1'`（`Key(49)`），因此此前的「注入没反应」长期是注入工具用错，不是功能问题。正确值是 Page Down **117**、Page Up **112**（到达 GTK 后是 `Key(65366)`，即 `0xFF56`）。

## X11 会话下预览窗口保持最上层（0.0.9）

**现象**：X11（kwin_x11 / KDE Plasma）会话里空格弹出的预览窗口不置顶——焦点在预览上，但点一下别的窗口它就被盖住；Wayland（niri）一直正常，因为那边靠 `quicklook.kdl` 的浮动窗口规则。

**根因**：GTK4 删除了 `gtk_window_set_keep_above`，X11 侧从未有对应实现，窗口就是普通层级。

**修法**（新文件 `crates/daemon/src/x11.rs`）：
- 按 EWMH 规范向根窗口发 `_NET_WM_STATE` 客户端消息（`data.l[0]=_NET_WM_STATE_ADD`、`data.l[1]=_NET_WM_STATE_ABOVE`、`data.l[3]=SOURCE_APPLICATION`），由窗口管理器把窗口钉在最上层；任何失败静默忽略。
- **为什么必须拿本窗口 XID 而不是按 pid 枚举**：第一版在 `connect_map` 回调里读根窗口 `_NET_CLIENT_LIST` 再按 `_NET_WM_PID == getpid()` 过滤——实测 map 回调触发的瞬间 KWin 还没把新窗口加进 `_NET_CLIENT_LIST`（列表里只有别的窗口），过滤结果是空集，消息根本没发出去。改用 GTK 内部符号 `gdk_x11_surface_get_xid`（X11 后端合在 `libgtk-4` 里，`nm -D` 确认导出）直接取本窗口 XID，此时 WM 已处理过 MapRequest，消息必然落在已管理的窗口上，无 race。为此给 daemon 显式加了 `gdk4 =0.11.5` 依赖（拿 `GdkSurface` 的 `ToGlibPtr` 类型）。
- 触发点：`Controller::new()` 末尾挂 `window.connect_map`，仅当 `x11::wanted(backend, detached)`（显示后端是 `GdkX11Display` 且不是独立窗口）时执行；独立预览对标 Wayland 上平铺的 Detached，应能被别的窗口盖住，不置顶。
- `crates/daemon/build.rs` 的 pkg-config 列表加 `x11`（缺 `libx11-dev` 时构建期明确报错）。

**隔离验证**（`dbus-run-session` 私有总线 + `DISPLAY=:0`，脚本按「启动前后 window id 差集」定位新窗口）：
- `xprop -id <WID> _NET_WM_STATE` = `_NET_WM_STATE _NET_WM_STATE_FOCUSED _NET_WM_STATE_ABOVE _NET_WM_STATE_STAYS_ON_TOP`（KWin 额外补了 STAYS_ON_TOP）。
- `wmctrl -a "DSH Tauri"` 把焦点切走后 `_NET_ACTIVE_WINDOW`=DSH，但 `_NET_CLIENT_LIST_STACKING` 尾部（最上层）仍是预览窗口——即别的窗口获得焦点、预览仍不被盖住，符合「置于顶层」。

**测试教训**：验证脚本里 `pkill -f quicklook-viewer` / `pgrep` 会自匹配包住同名字符串的外层 `bash -c`，把整个测试 shell 连同 dbus-run-session 一起杀掉，表现为「脚本莫名挂死」。清进程一律按 `/proc/<pid>/exe` 的 basename 判断后 `os.kill`。

**质量门**：134 项测试、clippy 0 警告、fmt 干净。
