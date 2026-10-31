# QuickLook for Linux — 需求文档 v0.1

> 目标：完整复刻 macOS Quick Look 的文件预览体验——**选中按空格秒弹预览、格式全覆盖、流畅缩放导航**。
> 首发环境：niri + Noctalia（Linux Mint 22.3 / Wayland），架构必须 DE/合成器无关、文件管理器无关。
> 立项动机：现有方案（nemo-preview / gnome-sushi / Kiview / Gloobus）实测体验均不达标（详见附录 A 竞品尸检）。

---

## 1. 核心交互（P0 — 没有这些就不是 QuickLook）

| # | 交互 | macOS 对应 | 说明 |
|---|------|-----------|------|
| 1 | 选中文件按**空格** → 弹出预览 | Space | 再按空格关闭（toggle 语义） |
| 2 | **Esc** 关闭 | Esc | |
| 3 | **↑/↓ 方向键** → 预览同目录上/下一个文件 | ↑/↓ | 跨文件连续浏览，无需关闭重开 |
| 4 | **非激活弹窗**：打开时不抢键盘焦点 | QL 窗口特性 | Wayland 下用 layer-shell 或 non-activating toplevel |
| 5 | 窗口跟随内容自适应大小，居中/贴近文件位置 | 同 | 最大不超过屏幕 70% |
| 6 | 标题栏信息：文件名 · 类型 · 大小 · 关键元数据 | 同 | |
| 7 | **Enter** → 用默认应用真正打开该文件 | Enter | |
| 8 | **多选文件 → 网格缩略图墙**（contact sheet），双击单张放大 | 多选+空格 | |
| 9 | **滚轮/捏合缩放**、拖拽平移、双击回到适应窗口 | 同 | 图片/PDF 通用 |
| 10 | 图片**旋转**（R / Shift+R） | ⌘R | |
| 11 | **全屏预览**模式（F） | ⌥⌘Y | |
| 12 | 预览中 **⌘C 复制文件本体**（进剪贴板可粘贴到文件管理器） | 同 | wl-copy 文件 URI |
| 13 | 右键/工具栏：打开方式（子菜单列出可选应用）、文件属性 | Open With | |
| 14 | 窗口可拖动、可固定（P 锁定不随导航关闭） | QL pin | |
| 15 | **Ctrl+D 窗口分离**：当前预览转成独立窗口，可平铺长期开着；关它之前/之后都能继续空格开新预览 | — | 独立进程 + 独立 app-id，主窗口随即关闭 |
| 16 | **统一预览键**：所有支持的文件管理器默认空格，可在设置里改成其他组合键 | Space | 一份配置驱动全部宿主；可分别开关是否劫持宿主原生预览 |

## 2. 格式支持矩阵

### P0（首发必须）
- **图片**：jpg / png / gif（含动画）/ webp（含动画）/ bmp / svg / ico
- **视频**：mp4 / mkv / webm / mov（可播即预览，交给后端解码器全家桶）
- **音频**：mp3 / flac / wav / ogg / m4a / opus
- **PDF**：多页滚动、页码、跳页、文本选择复制
- **文本**：txt / 全部源代码（语法高亮）/ 配置文件；**编码自动检测（UTF-8/GBK/GB18030——中文用户硬需求）**；大文件（>10MB）分段加载不卡
- **Markdown**：渲染预览（可切换源码/渲染）；```mermaid 流程图离线渲染（Mermaid 11），可用配置关闭
- **目录**：内容摘要（项数、总大小、近期修改）+ 内部文件快速浏览
- **压缩包**：zip / tar / 7z / rar 内容列表（大小/时间），双击进入预览包内文件
- **未知二进制**：文件头 hex + 可打印字符串摘要（file 命令风格）

### P1（第二迭代）
- tiff / heic / heif / avif / jxl
- 相机 RAW：cr2 / nef / arw / dng（内嵌 JPEG 优先提取，快）
- **设计源文件（对标 QuickLook 原生支持）**：
  - **.ai**（Illustrator v9+，现代格式本体是 PDF 容器）→ 路由给 PDF 预览器（poppler），零额外成本秒开；v8 及更早的 PostScript 老格式走 Ghostscript（P2）
  - **.psd**（Photoshop）→ **合成图层快路径**：只提取内嵌合成位图（composite image），不解析图层树——这正是 macOS 快的原因；实现优先级：纯 Rust `psd` crate → libvips → ffmpeg 解码器兜底；含图层列表侧栏（P2，仅枚举名称/尺寸，不渲染）
  - **.eps** → Ghostscript 转 PDF 后走 PDF 预览（结果缓存）
- HTML 渲染（webkit 层）
- RTF / JSON / XML / YAML 结构化视图（折叠树；JSON 默认源码）
- CSV / TSV 表格预览（可切源码）
- 字体预览（字形面板 + 示例文本）
- 视频音频**转码不兼容时的降级**（提示 + 元数据）

### P2（第三迭代 / 接受外部插件）
- Office：docx / xlsx / pptx —— **已实现**：直接解析 OOXML（docx 段落/标题/列表/表格；xlsx 工作表与单元格；pptx 放映顺序与文本），不调用 LibreOffice、不留转换产物；不做版式还原
- epub（目录+正文）、ics/vcf（日历/联系人卡片）
- 压缩包内深层预览（嵌套）—— **已实现**：Enter 逐层展开，Backspace/Esc 返回，最多 4 层

## 3. 性能硬指标（竞品全死在这里，这是本项目存在的理由）

| 指标 | 目标 | 手段 |
|------|------|------|
| 热启动弹出延迟 | **< 80ms**（人类感知"瞬间"） | 常驻 daemon + DBus 唤起（冷启动首次 < 400ms） |
| 同目录切换下一文件 | < 30ms | **邻近文件预加载**（当前文件 ±3 个的缩略图/头部提前解码） |
| 常驻内存 | < 60MB | daemon 不持有解码缓存，LRU 上限 200MB 可配 |
| 万行文本打开 | < 150ms | 分页加载 |
| 千张图目录浏览 | 方向键连按不落帧 | 缩略图懒加载 + 预取队列 |
| 渲染 | GPU 直出，无 CPU 软渲染路径 | 媒体走 libmpv（硬解），图片走 GPU 纹理 |

## 4. 各格式预览器功能细节（P0 部分）

### 图片
- 适应窗口 / 1:1 像素 / 自由缩放三态（0=适应）
- 动图（gif/webp）默认自动播放，可暂停在帧
- EXIF 侧栏（I 键）：相机、参数、GPS（有则显示，无则隐藏）
- 超大图（>100MP）渐进解码

### 视频/音频（统一于 libmpv）
- 播放/暂停（空格被占用→用 K 或点击）、进度条拖拽、音量/静音
- 倍速（< >）、逐帧（, .）、循环开关
- 音频显示封面 + 波形/频谱可视化（P1）
- 无损拖动定位（关键帧 seek）

### PDF
- 连续滚动 / 单页两态；页码输入跳页
- 页缩略图侧栏（T 键）
- 文本层选择与复制

### 文本/代码
- gtksourceview 级语法高亮（语言自动检测）
- 行号、自动换行开关（W）、等宽字体
- 编码检测失败时允许手动切换重载（E 键循环 GBK/UTF-8/GB18030/Latin1）

## 5. 插件架构（模仿 qlgenerator 生态）

- 内核只做：窗口/焦点/导航/缩放/DBus/调度
- 每种 MIME 一个 **previewer 模块**，注册表：`mimetype → handler`，通配符支持（`image/*`）
- 插件发现路径：内置 → `/usr/share/quicklook/plugins/` → `~/.config/quicklook/plugins/`
- 插件接口（v1）：`preview(path, widget_parent) -> widget | fallback_next`，声明支持权重（具体扩展名 > 通配）
- 未匹配 → 内置降级链：文本尝试 → hex → 元数据卡片

## 6. 集成点（多入口，互不依赖）

| 入口 | 方式 | 优先级 |
|------|------|--------|
| **CLI** | `quicklook <path>...`（dbus 激活 daemon） | P0——一切集成的地基 |
| **DBus 服务** | `org.quicklook.Preview` 的 `Preview(paths[])` / `Next()` / `Prev()` / `Close()` | P0 |
| **niri 全局键** | `Ctrl+Alt+P`：无文件管理器场景，fzf 选文件→预览（管道流） | P0 |
| **Nemo 动作** | `.nemo_action` + Shortcut 绑**空格**（选中集传入 %F）——需验证 Nemo 对动作快捷键的支持；不行则绑 `Ctrl+Space` | P0 |
| Dolphin / GNOME Files（Nautilus）/ Yazi | 可选原生接口、动作或插件；复用同一预览服务 | 当前范围 |
| Thunar 等其他 FM | 各自 action/service 定义 | P2 |
| Nemo 侧栏/上下文菜单 | "用 QuickLook 预览" | P1 |

## 7. 技术选型（建议案，开工前可复议）

**推荐：Rust + GTK4**
- libmpv（render API 嵌入窗口）→ 视频/音频/图片三合一 GPU 后端
- libpoppler-glib → PDF
- gtksourceview-5 → 代码高亮
- GTK4 图片/文本原生部件
- 构建目标：meson，安装进 `~/.niri-stack/prefix`（复用现有工具链与 g++/meson 环境）
- 备选否决理由：C++/Qt（用户环境忌 Qt 引入）；Python 原型（启动延迟不达标，竞品死因之一）；Clutter/GStreamer 栈（nemo-preview 死因）

**daemon 形态**：单实例 `quicklookd`，DBus 激活（.service 文件），空闲 10 分钟自动退出可配。

## 8. 里程碑

| 阶段 | 交付 | 验收 |
|------|------|------|
| **M1 弹得快** | daemon+DBus+CLI；图片/文本/PDF 三类弹窗；空格 toggle；niri `Ctrl+Alt+P` 集成 | 热启动 <80ms；三类格式秒弹 |
| **M2 用得爽** | 方向键导航+预加载；libmpv 媒体；缩放/平移/旋转；多选网格 | 切换 <30ms；视频秒开 |
| **M3 盖得全** | 插件注册表；目录/压缩包/编码检测/Markdown；Nemo 空格集成 | P0 格式矩阵全绿 |
| **M4 补得齐** | P1 格式；Office 缓存方案；元数据面板 | 按需排期 |

## 9. 非目标（明确不做）

- 文件管理（浏览/复制/移动/重命名）
- 编辑与标注（旋转除外——标注已有 satty 工作流）
- 缩略图磁盘缓存服务（那是 tumbler 的活；仅进程内 LRU）
- X11 主动适配（xwayland 下能跑即可，不专门优化）

---

## 附录 A：竞品尸检（为什么都"效果极差"）

| 竞品 | 死因 | 本项目对策 |
|------|------|-----------|
| nemo-preview | Clutter 老栈，冷启动慢，格式靠 GTK 各部件拼凑，视频弱 | 常驻 daemon + libmpv |
| gnome-sushi | 绑死 Nautilus；首次按键冷启动明显延迟 | DBus daemon 保活；FM 无关 |
| Kiview | Qt/Dolphin 生态；窗口动画与渲染质感差；通用格式覆盖薄 | GTK4 原生 Wayland；格式矩阵见上 |
| Gloobus | 停更气质，Wayland 支持存疑 | Rust/GTK4 全新实现 |
| swayimg/zathura 拼装 | 各自独立窗口、无统一交互、无空格语义 | 单窗口统一交互层 |

**根本结论**：macOS QuickLook 的"快+全"来自 OS 级 qlgenerator 体系与常驻服务。Linux 复刻的关键 = **常驻 + 预加载 + 每格式最快后端 + 统一交互层**，四者缺一即"效果极差"。

## 附录 B：macOS QuickLook 行为核对清单（防止漏仿）

- [x] 空格开关 toggle
- [x] 方向键连续浏览
- [x] 非激活弹窗
- [x] 缩放/平移/适应
- [x] 旋转
- [x] 全屏模式
- [x] 多选 contact sheet
- [x] 视频播放控制（进度/音量/倍速/逐帧）
- [x] PDF 翻页/缩略图/文本复制
- [x] 复制文件/打开方式/共享
- [x] 插件生态（qlgenerator → 本项目 plugin registry）
- [ ] 标注 Markup（macOS 有，本项目列 P2 之外——已有 satty，不重复造）
- [ ] 音视频修剪 Trim（同上，P2 外）

## 10. 当前范围补充：独立使用与四种文件管理器

- 预览核心不依赖文件管理器。支持 CLI、DBus、应用自己的选文件窗口；不要求安装或运行任何特定文件管理器。
- 可选兼容范围：Nemo、Dolphin、GNOME Files（Nautilus）、Yazi，四种都复用同一 Rust 服务。
- 兼容入口按宿主公开能力提供，不能把右键动作等同于所有版本都具备原生空格钩子。本机 Dolphin 23.08 采用右键；26.04 起支持自定义动作快捷键。GNOME 原生预览接口只传一个 URI，多选由脚本入口传入；Yazi 可选择 Ctrl+Space 或空格。
- 配置和验证边界见 docs/FILE_MANAGERS.md；不重启用户已有的文件管理器窗口。
