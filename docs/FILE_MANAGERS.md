# 文件管理器接入

Quick Look 可以独立运行，文件管理器是可选入口。Rust 核心负责预览，适配器只传选中的本地文件 URI。

## 设置与启用

打开应用菜单里的「Quick Look 设置」，或运行 `quicklook --settings`：

1. 选择要接入的文件管理器。
2. 需要预览快捷键时打开「启用预览按键接管」。新配置默认关闭；已有设置不被重装覆盖。
3. 点击独立的「应用文件管理器接入 / 注入」按钮。普通「保存设置」不写宿主配置。
4. 正常退出并重新打开宿主。Caja 安装 python3-caja 后可以直接运行 caja；Thunar / 改键后的 Nemo 从应用菜单或适配启动器打开。通过适配启动器遇到未加载当前适配器的旧实例时，会询问是否重启；取消不会关窗。

「撤销全部接入」会撤掉托管文件和块，并恢复备份的 desktop、原生预览服务。它不关闭文件管理器。正在运行的适配器要在对应进程退出后释放。

命令行对应入口：

```bash
quicklook --apply-hosts --dry-run
quicklook --apply-hosts
quicklook --remove-hosts
quicklook --launch-host caja
```

`--remove-hosts` 撤销当前接入，不改个人偏好文件；再次应用会按保存的偏好接入。

## 能力与兼容范围

| 文件管理器 | 入口 | 按键方式与限制 |
|---|---|---|
| Nemo 6.6 | 右键、预览键 | 空格走公开 `org.nemo.Preview`；其他键通过已验证构建的可选适配器，未知构建不注入 |
| Caja 1.26 | 右键、预览键 | python3-caja 用户扩展加载 Rust 适配，普通 caja 命令已实测；无扩展时可用显式启动器 |
| Thunar 4.18 | 右键、空格（已实测） | 自定义动作负责右键；按键需进程内适配器 |
| Dolphin 23.08 | 右键、多选 | **目前无法支持空格预览**：空格是宿主自己的打开/重命名行为，没有本项目可用的接管入口 |
| GNOME Files / Nautilus 46.4 | 原生空格（已实测）、多选脚本 | Sushi 兼容协议接空格；Ctrl/Alt 组合接脚本加速键 |
| Yazi 26.9 | 插件、预览键、悬停联动 | 使用公开 Lua/事件接口；保留已有用户同键绑定 |

不是任意宿主版本的兼容保证。Nautilus 46.4 使用发行版 deb 解包到临时目录的真实窗口补测，通过原生空格开/关；没有系统安装它。Thunar 4.18.8 也已通过真实按键验证。图形测试运行在 Xephyr 隔离会话，不能代替每种 Wayland 合成器的体验验收。

## Caja 修复与使用

Caja 单实例：新起的进程可能只向旧实例发请求，随后退出。因此只设置 LD_PRELOAD 环境而不处理已有实例，看起来会像没有生效。启动器按当前会话总线定位主进程，并检查其映射的适配器 inode；旧版本或没有适配器时提示重启。

用户主动接入后生成：

```text
$XDG_DATA_HOME/quicklook/launchers/quicklook-caja
$XDG_DATA_HOME/applications/caja*.desktop
$XDG_DATA_HOME/caja-python/extensions/quicklook.py
```

应用菜单和「打开文件夹」可能使用不同的 Caja desktop 条目，因此覆盖系统存在的五个入口，并保留原有多语言名称、动作及参数。原本有用户覆盖时先保存 `.quicklook-original`，撤销时恢复。

按键适配使用 Caja 导出的 `caja_view_get_selection` 等符号，按实际对象类型取 URI；符号缺失时放弃接管。选择列表按 transfer-full 释放对象引用与链表。地址栏、搜索框、重命名输入框中的按键不会拿文件区的选择来预览。

python3-caja 按 [官方加载逻辑](https://github.com/mate-desktop/python-caja/blob/master/src/caja-python.c)在启动时导入用户扩展，扩展调用 Rust 库导出的 quicklook_caja_install；不需要先打开右键菜单，也不依赖启动时的 LD_PRELOAD。Rust 入口核对进程和接管配置，库保持驻留；共享 GTK 标记保证扩展与显式加载同时存在时只安装一个按键回调。关闭接管或缺少适配库时，扩展仍保留右键菜单。已打开的旧进程需要退出重开。

右键扩展通过 `Caja.MenuProvider` 创建动作。菜单使用 `--show`，重复点击保持显示；回调通过真实 `Caja.MenuItem.activate()` 回归，包含信号参数数量检查。

## 启动器与系统边界

可选适配器在包内的 `/usr/lib/quicklook`（源码自装为 `<prefix>/lib/quicklook`）。运行时生成的启动器统一在用户数据目录；不会写 `/usr/bin`，不写 `/etc/ld.so.preload`，也不创建 `~/.local/bin/caja` 或 `thunar` 同名命令覆盖。旧版留下的对应软链接会在重新应用时清理，只匹配目标属于本工具的链接。

Caja 已安装 python3-caja 并应用接管时，直接运行 `/usr/bin/caja` 或 `caja` 会经用户扩展加载适配。缺少 python3-caja 的 Caja、以及直接运行的 `/usr/bin/thunar` 仍需适配启动器或相应已接入的启动路径。Nemo 默认空格走原生服务，不需要注入。

适配只在已加载库的进程中生效。宿主升级后仍需复测，尤其是依赖私有符号的 Caja 和指纹限定的 Nemo 改键适配；没有把 LD_PRELOAD 说成稳定的文件管理器插件 API。

Thunar 的接入还覆盖用户级 `org.xfce.Thunar` / `org.xfce.FileManager` D-Bus 激活，以及 `~/.config/systemd/user/thunar.service.d/quicklook.conf`。应用与撤销均只重新读取定义，不自动开关宿主；已有同名用户文件会备份和恢复。不会占用通用 `org.freedesktop.FileManager1`，避免改变 Dolphin 等默认文件管理器的选择。

niri 自定义快捷键若直接执行 `thunar`，应改用 `quicklook --launch-host thunar`。

这类「绕开 desktop 覆盖、直接 exec 裸 `thunar`」的启动脚本由 `--apply-hosts` 统一迁移（0.0.1-8 起）：
查找 `--prefix>/bin/launch-file-manager` 与 `~/.niri-stack/prefix/bin/launch-file-manager` 两处，
仅在脚本里**恰好有一行**那条已知命令时插入受管块，块内先尝试 `exec` 带 `LD_PRELOAD` 的启动器，
失败则落回原命令；原来的 `-u GTK_IM_MODULE` 与 `GDK_BACKEND=wayland` 环境设置原样保留。
关闭接管或 `--remove-hosts` 只剥掉该块，恢复成原脚本。命令形式不认识的脚本一律不改，只在报告里提示。
此前只有源码安装器的 `--with-niri` 做这件事，deb 路径从不做——从源码版迁到 deb 后空格失效正是这个原因。

## 其他宿主

Dolphin：`$XDG_DATA_HOME/kio/servicemenus/quicklook.desktop`。遵循 [KDE 服务菜单接口](https://develop.kde.org/docs/apps/dolphin/service-menus/)，以 `%U` 传 URI，设置可执行位并刷新 KSycoca 缓存。动作使用 Toggle，使绑定空格后能再次关闭。

Dolphin 的空格支持按版本区分：[官方 26.04 说明](https://blogs.kde.org/2026/05/03/dolphin-26.04-release/)确认服务菜单可以绑定快捷键，源码还要求 KIO >= 6.24。也就是说 **26.04 之前的所有 Dolphin 版本（含 23.08）目前无法支持空格预览，只能右键**。新版在「配置快捷键 → 上下文菜单动作」找到 Quick Look 并设为空格；本工具不会覆盖宿主已有快捷键。该方案已核对 Dolphin 26.04 / KIO 的官方源码，本机没有这套新依赖栈，未宣称真机验收。

本机 23.08.5 实测空格不会进入 Quick Look。它没有该服务菜单快捷键接口；不是「Qt 程序永远不能做」，而是旧版要回移植上述功能或另做进程内 Qt 适配。本轮没有对旧版做这些修改，也没有升级用户的 KDE/Qt。

Nautilus：脚本位于 `nautilus/scripts/Quick Look`，从 `NAUTILUS_SCRIPT_SELECTED_URIS` 读取完整多选。可选的 `org.gnome.NautilusPreviewer.service` 接原生预览，兼容 [Previewer2 协议](https://github.com/GNOME/sushi/blob/master/data/org.gnome.NautilusPreviewer2.xml)的三参数和带 token 的四参数调用。原有用户级服务会备份/恢复；Sushi 已持有总线时不强抢，需等原服务退出。已有相同 Ctrl/Alt 脚本加速键会保留，不再只检测 Ctrl+Space。

Thunar：动作插入 `~/.config/Thunar/uca.xml` 的 `<actions>` 根，命令路径按 XML 与 shell 分层转义，用户动作保留。

Yazi：插件位于 `~/.config/yazi/plugins/quicklook.yazi/`，init.lua / keymap.toml 只修改托管块。行内数组无法安全追加时保留原文件并输出合并片段；已有按键不覆盖。关闭预览后的悬停事件不会重开窗口。

## 验证

```bash
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/test-caja-adapter.py --profile release
python3 scripts/test-nemo-adapter.py --profile release
python3 scripts/test-deb.py dist/quicklook_V0.0.9_amd64.deb
```

Caja 在 Xephyr + marco + 私有 D-Bus 会话中验证空格、改键、多选、地址栏、菜单回调及选择引用释放。不会重启用户会话中的文件管理器。Nemo 的测试选择器使用临时 keyfile 设置强制列表视图，不依赖用户当前布局。

跨文件管理器复制使用独立 Wayland 剪贴板持有进程，提供 URI、GNOME copied-files 和 KDE cutselection 目标；无 data-control 协议时回落 wl-copy，不承诺所有合成器的完整多 MIME 互通。

## 连续空格（2026-10-26）

已删除 viewer 的 400 ms 切换冷却与适配器 120 ms 时间锁存。宿主按下/松开事件维护真实状态，重新获得焦点立即清掉旧状态。窗口收到自己的完整 Space 按下/松开后关闭，避免只收到重复按下就反复抢回宿主焦点；两次按键之间没有最小等待时间。

scripts/test-space-keys.py 用真实宿主各验证连续五组开/关，以及焦点切到预览窗口后的下一次 Space。Nemo 默认空格路径没有使用注入来代替公开原生接口。Yazi 则用真实 PTY 发送空格。

```bash
python3 scripts/test-space-keys.py --profile release
# 测试临时解包的 Nautilus（含对应库和 schemas），不要求系统安装：
python3 scripts/test-space-keys.py --profile release --nautilus /absolute/path/to/extracted-prefix
```

新配置仍需选择「启用预览按键接管」并应用。Caja / Thunar 必须加载适配器，Yazi 要加载插件与按键配置；已有冲突绑定、已运行的原生预览服务、未重启的旧适配器不能被当作“已经接入”。

## 按键只在文件视图里生效（0.0.3-2 起放宽）

适配器从「当前焦点控件」向上找文件视图，找不到就退回到窗口里的第一个文件视图——
只要焦点不在输入框/文本框里。所以点过侧边栏书签、工具栏或状态栏之后，空格仍然
作用于当前选择；而地址栏、搜索框、行内重命名里按空格照旧是打字，不会触发预览。

「空格还按着」的锁存除了在宿主窗口 `focus-in` / `focus-out` 时清零，还有 400ms
兜底时限：预览窗口抢走焦点导致抬起事件丢失时，锁存会自己松开，不会让下一次空格
被误当成自动重复。
