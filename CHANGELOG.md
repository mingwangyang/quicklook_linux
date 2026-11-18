# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.0.19] - 2026-11-18

### 修复

- 修复文件夹预览第二次唤起后空格无法关闭：目录列表的 `GtkListView` 会保留键盘焦点，第二次显示时把空格当成列表输入；每次 `present_preview` 后把焦点拉回内容容器，并在 120ms 后再次矫正，文件夹连续开关恢复正常。
- 再次确认 Markdown/HTML 在 WebKit 渲染过程中按空格可以关闭（WebView 独立按键控制器）。

### 性能

- `quicklookd` 的预热改为后台 `spawn`，不再在 broker 主线程等待 viewer 注册：即使 viewer 首次初始化耗时较长，`Status` 仍能在十几毫秒内响应，避免第一次 `Show` 因预热阻塞而超时。

## [0.0.18] - 2026-11-17

### 修复

- 修复文件夹第二次唤起、Markdown 渲染过程中按空格偶尔关不掉：Thunar/Caja/Nemo 适配器的正常路径不再同步查询 broker 的 `Status`，空格直接走 `Toggle`；只有焦点不在文件视图或宿主不活动时才查询一次并关闭。Markdown 的 WebView 单独挂按键控制器，渲染中按空格也能关闭。

### 性能

- `quicklookd` 在登录自启动时预热一个隐藏 viewer（隐藏生命周期 1 小时），首次真正唤起不再承担进程启动与 GTK/WebKit 初始化：实测 CLI 返回约 10ms、首帧约 154ms（此前冷启动约 4.2s）。
- `viewer_grace_secs` 默认从 3 秒提高到 30 秒，可设 3–600 秒；关闭后短时间内重开继续复用 viewer。

## [0.0.17] - 2026-11-17

### 修复

- 修复在预览窗口的标题栏、格式栏或底部状态胶囊上点击后，再按空格无法关闭的问题：这些区域不属于内容区，点击会把键盘焦点交给按钮；现在分别给标题栏、格式栏和状态胶囊挂 Capture 阶段的点击手势，点击释放后把焦点还给内容区，空格始终由预览窗口处理。

## [0.0.16] - 2026-11-17

### 修复

- 修复从 Thunar 按空格打开预览后，第二次按空格仍被适配器的 `HoldLatch` 吞掉、必须先点一下预览窗口才能关闭：Thunar/Caja 的“预览已可见就关闭”分支不再要求按键锁存已释放，只用 80ms 的自动重复窗口过滤长按重复；同时把宿主 active 检查移到关闭分支之后，预览已显示时即使宿主不是活动窗口也能关闭。

## [0.0.15] - 2026-11-17

### 修复

- 修复从文件管理器用空格唤起预览后，必须先点一下预览窗口才能再按空格关闭：窗口首次呈现时显式请求键盘焦点（GTK grab 加 GDK toplevel focus，120ms 后重试），不再依赖合成器的自动聚焦时机。

## [0.0.14] - 2026-11-16

### 修复

- 修复从文件管理器用空格唤起预览后不能直接空格关闭：首次 `Toggle` 不再被「刚启动 1.5 秒内」的 inflight 判定误判成连按而立即关闭；Thunar / Caja 适配器只在自身窗口处于活动状态时处理空格，避免预览窗口已经获得焦点后宿主重复触发开关。

## [0.0.13] - 2026-11-16

### 修复

- 修「不止 PDF」的空格关闭问题：音频/视频控制栏不再使用会弹出独立表面的 `GtkDropDown`，倍速与可视化改为循环按钮；打开预览的「打开方式」菜单里空格也能关闭；窗口可见时任何 `Toggle` 都按关闭处理，不再因为宿主改选文件或未同步选区而变成重开。
- Thunar / Caja / Nemo 适配器在空格按下时先问 broker：预览已经可见就直接 `Close`，不再依赖焦点是否落在文件视图或按键锁存是否收到抬起事件；焦点停在路径栏、搜索框或工具栏时也能关闭。

## [0.0.12] - 2026-11-16

### 修复

- 修复「有时候空格关闭不了」：焦点落在 PDF 页码输入框或格式栏输入控件时，空格被 `editing` 分支直接放行；现在空格在输入框、按钮和文本视图上同样是关闭键（Ctrl/Alt+空格除外）。

## [0.0.11] - 2026-11-16

### 优化

- 预览窗口的内容区约占窗口九成：标题栏压成单行（文件类型/大小改为标题悬停提示），底部状态栏改为浮在内容上的胶囊、几秒后自动淡出；图片与 PDF 按页面比例确定窗口，PDF 不再固定成横向窗口、两侧留大片空白。
- 显示区域背景色可按常见格式配置（PDF / 图片 / 视频 / 音频 / 文档 / 表格 / 文本 / 其它八类），默认值向对应经典应用看齐：PDF `#333333`（Evince / Acrobat）、图片 `#1f1f1f`（Gwenview / EOG）、视频 `#000000`（mpv / VLC）、音频 `#1e1e1e`（Audacity）、文档 `#e6e6e6`（LibreOffice 桌面灰）、表格 `#ffffff`（Calc 单元格白）；设置窗口「外观」页可自定义或跟随主题，打开下一个文件时生效。

## [0.0.10] - 2026-10-31

### 修复

- 「单页模式」按钮不再被默认连续滚动顶回：默认开启连续滚动时，用户切到单页会在下一次加载被自动切回（0.0.7 回归）；现在用户的选择在同一文件内持续生效，换文件后才回到默认。

### 文档

- 明确 Dolphin 目前无法支持空格预览：空格是宿主自己的打开/重命名行为，23.08 无预览热键接口，26.04+ 需 KIO ≥ 6.24 才能在宿主手动绑定
- 重写开发环境文档：去除本机路径、补充最小编译集与运行时依赖的区分表、其他发行版包名对照（Fedora/Arch）、Rust 工具链调整与镜像加速、NixOS/容器说明、常见问题
- 使用指南新增「实测过的桌面环境」一节，明确各桌面/合成器/托盘宿主的验证状态与边界

### CI

- actions 升级到 Node 24 兼容版本；发版工作流自动构建 deb 并附到 Release
- 测试不再依赖真机 /usr/bin/nemo，CI 可完整验证改键链路

## [0.0.9] - 2026-10-31

### 修复

- X11 会话下预览窗口保持最上层

### 变更

- 仓库开源化：清理个人信息、固化 `main` + `dev` 分支模型

## [0.0.8] - 2026-10-28

### 修复

- PDF 连续滚动模式下 Page Up / Page Down 真正翻页

## [0.0.7] - 2026-10-28

### 新增

- PDF 默认连续滚动并按页缩放
- 设置窗口分类重组、窗口大小与自动检测选项
- PDF 右键拖动

### 修复

- 全屏提示与 F 键行为

## [0.0.3] - 2026-10-28

### 新增

- 托盘右键菜单新增「关于」并显示版本号

## [0.0.1] - 2026-10-26

### 新增

- 首个可用版本：P0/P1 预览路径（图片 / 媒体 / PDF / 文本 / Markdown / Office / 压缩包）、
  统一预览键、图形设置窗口、窗口分离（Ctrl+D）、CLI / DBus、niri 集成
- 文件管理器接入：Nemo、Dolphin、GNOME Files（Nautilus）、Thunar、Caja、Yazi
- 常驻进程 `quicklookd` 带托盘图标；提供 deb 安装包

[Unreleased]: https://github.com/mingwangyang/quicklook_linux/quick_look_linux/compare/V0.0.9...HEAD
[0.0.9]: https://github.com/mingwangyang/quicklook_linux/quick_look_linux/compare/V0.0.8...V0.0.9
[0.0.8]: https://github.com/mingwangyang/quicklook_linux/quick_look_linux/compare/V0.0.7...V0.0.8
[0.0.7]: https://github.com/mingwangyang/quicklook_linux/quick_look_linux/compare/V0.0.3...V0.0.7
[0.0.3]: https://github.com/mingwangyang/quicklook_linux/quick_look_linux/compare/V0.0.1...V0.0.3
[0.0.1]: https://github.com/mingwangyang/quicklook_linux/quick_look_linux/releases/tag/V0.0.1
