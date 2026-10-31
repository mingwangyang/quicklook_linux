# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 修复

- 「单页模式」按钮不再被默认连续滚动顶回：默认开启连续滚动时，用户切到单页会在下一次加载被自动切回（0.0.7 回归）；现在用户的选择在同一文件内持续生效，换文件后才回到默认。

### 文档

- 明确 Dolphin 目前无法支持空格预览：空格是宿主自己的打开/重命名行为，23.08 无预览热键接口，26.04+ 需 KIO ≥ 6.24 才能在宿主手动绑定
- 重写开发环境文档：去除本机路径、补充最小编译集与运行时依赖的区分表、其他发行版包名对照（Fedora/Arch）、Rust 工具链调整与镜像加速、NixOS/容器说明、常见问题
- 使用指南新增「实测过的桌面环境」一节，明确各桌面/合成器/托盘宿主的验证状态与边界

### 计划中（P2）

- PSD 图层树预览
- PCManFM 等更多文件管理器适配

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
