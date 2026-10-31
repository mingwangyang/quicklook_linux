# deb 安装与分发

当前应用版本为 **V0.0.10**（`Cargo.toml` 的 `version` 字段）。deb 版本与应用版本完全一致，不再带 `-N` 修订后缀：
应用版本从 0.0.1 走到 0.0.3，托盘右键菜单新增「关于 Quick Look」（弹窗里写版本号）；
0.0.9 X11 会话下预览窗口保持最上层（EWMH _NET_WM_STATE_ABOVE）；
0.0.10 修「单页模式」按钮被默认连续滚动顶回的回归；
0.0.8 PDF 连续滚动模式下 Page Up / Page Down 真正翻页（一次一屏，缩放后不漂移）；单页模式仍是换页；
0.0.7 PDF 默认连续滚动并按页缩放；全屏显示「按 F / Esc 退出」提示并修好 F 还原失灵；设置窗口分类导航、可调默认窗口大小、一键自动检测文件管理器；PDF 右键拖动平移；
0.0.6 修「空格有时候关不掉」：把「自动重复」判定从 400ms 收紧到 150ms，窗口尚未画出时的第二次 Toggle 直接关闭；
0.0.5 启动器不再弹「需要重启 Thunar？」，未加载适配器的实例改为静默重启一次；
0.0.4 新增 CSV / TSV 表格预览（分隔符嗅探、数值右对齐、表格/源码双模式），JSON 默认源码视图；
0.0.3-2 修好「空格关闭有时候失效」（焦点锁存自愈、焦点不在视图时回落到窗口视图）；
并含 0.0.1-8 修好的启动脚本迁移、0.0.1-6 的登录自启、0.0.1-5 的安装时自动接入与
niri 规则托管、0.0.1-3 起的 Markdown Mermaid 渲染。

## 构建

```bash
source "$HOME/.cargo/env"
python3 scripts/build-deb.py            # 版本号取自 Cargo.toml，不再传 --version
```

构建工具需要 dpkg-dev、binutils 和 DEVELOPMENT.md 所列开发依赖。产物在 dist/，附 SHA-256 校验文件。构建脚本根据 ELF 与发行版符号表自动生成运行时依赖，不把开发包列为运行时依赖。

## 安装

```bash
sudo apt install ./dist/quicklook_V0.0.10_amd64.deb
# 装完补上用户级接入（文件管理器 + niri 窗口规则）
quicklook --apply-hosts
```

用 apt 安装本地包会补齐依赖。应用菜单中有「Quick Look」和「Quick Look 设置」；前者可以独立选文件预览。

安装后进入设置，选择需要接入的文件管理器。希望接管预览键时启用「预览按键接管」，再点击独立的「应用文件管理器接入 / 注入」按钮。默认不接管原有按键。

Caja、Thunar 和改键后的 Nemo 需要正常退出旧实例，再重新打开以加载进程内适配器。如果通过适配启动器打开，启动器会询问是否重启，取消则不关闭窗口。终端可用：

```bash
quicklook --launch-host caja
```

Caja 安装 python3-caja 后，由用户扩展加载 Rust 按键适配，普通 `caja`、`/usr/bin/caja` 与应用菜单都可使用空格；仍需用户启用并应用接管。该依赖在 deb 的 Suggests 中，选择 Caja 接入但尚未安装时可执行 `sudo apt install python3-caja`。没有该扩展时显式启动器仍可使用。不会通过 PATH 同名文件替换系统命令。

Thunar 接入包含用户级 D-Bus 激活入口和 thunar.service 的单独配置片段，避免后台实例先启动而未加载适配器。应用接入只重新读取服务定义，不启动或重启 Thunar。自定义桌面快捷键直接执行 `thunar` 时仍会绕过这些入口，应改为 `quicklook --launch-host thunar`，或使用被覆盖的应用菜单。

源码安装的 `--with-niri` 会修补安装前缀内已知的 `launch-file-manager` 脚本，保留 Wayland/输入法处理，并用托管块转交适配启动器；关闭接管后自动走原命令，源码卸载时移除该块。deb 安装不会修改用户自定义脚本。

程序文件安装在 /usr/bin、/usr/lib/quicklook 和 /usr/share。设置、启动器和用户级集成写入当前用户的 XDG 目录；应用运行时不写 /usr。包安装不启动图形进程、不重启文件管理器、不遍历用户家目录，也不向 /etc/ld.so.preload 写内容。

## 从源码自装版迁移

如果此前装到 ~/.niri-stack/prefix，先在旧设置里撤销接入，再运行源码的 scripts/uninstall-local.py，之后安装 deb。两个版本同时保留时，用户级 desktop 和 D-Bus 文件优先于系统文件，可能继续调用旧版本。卸载脚本支持 --dry-run；迁移前先检查结果。

## 卸载

先在设置中点击「撤销全部接入」，恢复备份的宿主入口并移除自己的托管块，再卸载软件包：

```bash
sudo apt remove quicklook
```

deb 卸载遵循系统包与个人设置的边界，不主动清除任何用户家目录。个人配置可留给重装使用。已加载进旧文件管理器的适配器会在用户正常退出该进程后释放。

## 验证边界

scripts/test-deb.py 检查包文件、依赖解析、解包后无管理员权限的接入与撤销、只读安装前缀和原始配置恢复；apt-get -s 验证当前系统的安装依赖。它不等同于跨发行版安装验收。真实 Caja 的端到端测试见 scripts/test-caja-adapter.py。

## 维护者脚本

包内含 `postinst` 与 `prerm`（源文件在 `packaging/debian/`），都不启动 GUI、不写系统配置：

- `postinst configure`：以发起安装的用户身份补一次它自己托管的接入文件。判断依据是 `$HOME/.config/quicklook/config.toml` 里是否 `hijack_native = true`：是则完整 `--apply-hosts`，否则只 `--apply-window-rules`（仅窗口规则）。
- `prerm remove|purge`：撤销接入（`--remove-hosts`），避免留下指向已删二进制的启动器。升级不触发。

用户识别优先级为 `PKEXEC_UID` → `SUDO_USER`，都拿不到就打印提示并跳过——不猜测、不批量处理其他用户。apt 的输出里会直接看到这些命令的结果，出问题时便于排查。
