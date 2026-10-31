#!/usr/bin/env python3
"""Install this app into the user's prefix and record every changed path.

Everything the app owns is written through a transaction: each pre-existing file
is backed up once, replacements are atomic, and any failure before the post-install
check restores the previous state. User files (niri config, host integration files)
are only ever changed through marked blocks or validated rewrites.
"""
import argparse
import datetime
import filecmp
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import niri_launcher

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--debug", action="store_true", help="install the development build")
parser.add_argument("--with-niri", action="store_true", help="add a validated app rule and picker binding")
parser.add_argument("--with-file-managers", action="store_true", help="install file-manager integrations (Nemo, Dolphin, Nautilus, Thunar, Caja, Yazi)")
parser.add_argument("--no-autostart", action="store_true", help="do not start the tray broker at login")
parser.add_argument("--no-start", action="store_true", help="do not (re)start the resident broker now")
parser.add_argument("--dry-run", action="store_true", help="show what would change without writing")
args = parser.parse_args()
if os.geteuid() == 0:
    raise SystemExit("Run as the desktop user, without sudo.")
prefix = Path(os.environ.get("QUICKLOOK_PREFIX", Path.home() / ".niri-stack/prefix"))
def xdg_path(name, fallback):
    path = Path(os.environ.get(name, ""))
    return path if path.is_absolute() else Path.home() / fallback


data = xdg_path("XDG_DATA_HOME", ".local/share")
config = xdg_path("XDG_CONFIG_HOME", ".config")
cache = xdg_path("XDG_CACHE_HOME", ".cache") / "quicklook"
if not prefix.is_absolute():
    raise SystemExit("QUICKLOOK_PREFIX 必须是绝对路径，未改动任何文件。")
stamp = datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).strftime("%Y%m%dT%H%M%S%z")


def desktop_arg(value):
    """桌面条目 Exec 字段的转义：反斜杠、引号、反引号、美元符号，再整体加双引号。"""
    text = str(value).replace("\\", "\\\\").replace("\"", "\\\"").replace("`", "\\`").replace("$", "\\$")
    text = text.replace("\\", "\\\\").replace("%", "%%")
    return f'"{text}"'


def run(command, timeout=10):
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        return subprocess.CompletedProcess(command, 127, "", str(error))


def bus_available():
    return bool(os.environ.get("DBUS_SESSION_BUS_ADDRESS")) or (
        Path(os.environ.get("XDG_RUNTIME_DIR", "")) / "bus"
    ).exists()


class Transaction:
    def __init__(self):
        self.entries = []  # (path, backup-or-None), in write order
        self.record = {"files": [], "backups": []}
        self.backed_up = set()

    def _prune(self, path):
        """同一份文件只留最近一次备份：以前每次安装都留一份，Nautilus 脚本目录里那些
        沉默的 .quicklook-backup-* 还会被当成额外菜单项列出来。"""
        for old in sorted(path.parent.glob(path.name + ".quicklook-backup-*"))[:-1]:
            old.unlink(missing_ok=True)

    def _settle_backups(self, path):
        """内容没变时不写新备份，但更早的重复备份照样要收干净：一次安装成功后，
        同一个文件最多只剩一份备份，卸载和排查时才分得清哪份是当前的。"""
        if not args.dry_run and path.exists():
            self._prune(path)

    def _backup(self, path):
        if path in self.backed_up:
            return
        self.backed_up.add(path)
        if path.exists() or path.is_symlink():
            backup = path.with_name(path.name + ".quicklook-backup-" + stamp)
            shutil.copy2(path, backup, follow_symlinks=False)
            # 先建新的再收旧的：这样任何时刻都还剩一份，且一定是最新的那份。
            self._prune(path)
            self.record["backups"].append(str(backup))
            self.entries.append((path, backup))
        else:
            self.entries.append((path, None))

    def _atomic_write(self, path, writer, executable=False):
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(path.name + ".installing")
        writer(temporary)
        if executable:
            temporary.chmod(0o755)
        temporary.replace(path)

    def write(self, path, text, executable=False):
        if path.is_file() and path.read_text() == text and (not executable or os.access(path, os.X_OK)):
            self.record["files"].append(str(path))
            self._settle_backups(path)
            return
        self.record["files"].append(str(path))
        if args.dry_run:
            print(f"会写入 {path}")
            return
        self._backup(path)
        self._atomic_write(path, lambda temporary: temporary.write_text(text), executable)

    def write_bytes(self, path, payload):
        if path.is_file() and path.read_bytes() == payload:
            self.record["files"].append(str(path))
            self._settle_backups(path)
            return
        self.record["files"].append(str(path))
        if args.dry_run:
            print(f"会写入 {path}")
            return
        self._backup(path)
        self._atomic_write(path, lambda temporary: temporary.write_bytes(payload))

    def copy_binary(self, source, target):
        if target.is_file() and filecmp.cmp(source, target, shallow=False) and os.access(target, os.X_OK):
            self.record["files"].append(str(target))
            self._settle_backups(target)
            return
        self.record["files"].append(str(target))
        if args.dry_run:
            print(f"会安装 {target}")
            return
        self._backup(target)
        self._atomic_write(target, lambda temporary: shutil.copy2(source, temporary), executable=True)

    def rollback(self):
        failures = []
        for path, backup in reversed(self.entries):
            try:
                if backup is not None and backup.exists():
                    temporary = path.with_name(path.name + ".restoring")
                    shutil.copy2(backup, temporary)
                    if os.access(backup, os.X_OK):
                        temporary.chmod(0o755)
                    temporary.replace(path)
                else:
                    path.unlink(missing_ok=True)
            except OSError as error:
                failures.append(f"{path}: {error}")
        if failures:
            print("回滚未完全成功：\n  " + "\n  ".join(failures))


transaction = Transaction()
profile = "debug" if args.debug else "release"
binaries = ("quicklook", "quicklookd", "quicklook-viewer", "quicklook-settings", "quicklook-nautilus-bridge", "quicklook-clipboard")
sources = {name: ROOT / "target" / profile / name for name in binaries}
for name, source in sources.items():
    if not source.is_file():
        raise SystemExit(f"Missing {source}; run cargo build {'--debug' if args.debug else '--release'} --workspace first.")
# Preflight anything that can fail without touching the disk. A missing source or
# an unwritable integration path must abort before the first file is replaced.
cli = prefix / "bin/quicklook"
daemon = prefix / "bin/quicklookd"
if args.with_niri and not (config / "niri/config.kdl").is_file():
    raise SystemExit("找不到 ~/.config/niri/config.kdl；未改动任何文件。")
if args.with_file_managers and cli.is_file():
    supported = "--dry-run" in run([str(cli), "--apply-hosts", "--help"], timeout=5).stdout
    if supported:
        preview = run([str(cli), "--apply-hosts", "--dry-run"], timeout=30)
        if preview.returncode != 0:
            raise SystemExit(f"文件管理器集成预检失败：{preview.stderr.strip() or preview.stdout.strip()}")
        for line in preview.stdout.splitlines():
            if not line.startswith("写入 "):
                continue
            path = Path(line[len("写入 "):])
            probe = path
            while not probe.exists() and probe != probe.parent:
                probe = probe.parent
            if not os.access(probe, os.W_OK | os.X_OK):
                raise SystemExit(f"没有写权限：{probe}（集成文件 {path} 无法写入）")


def replace_include(text, name):
    """把 niri 配置里指向 quicklook.kdl 的 include 换成临时文件名。"""
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.strip().startswith("include") and "quicklook.kdl" in line:
            lines[index] = f'include "{name}"'
            return "\n".join(lines) + ("\n" if text.endswith("\n") else "")
    return text.rstrip() + f'\n\n// Quick Look file preview\ninclude "{name}"\n'


def set_config_option(path, key, value):
    """把 ``key = value`` 写进配置，保留用户已有的其它项。

    自启是用户可见的开关，装完设置窗口里应当显示为已开启，所以这里写真实配置
    而不是只留一个 desktop 文件。
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    line = f"{key} = {value}"
    lines = path.read_text().splitlines() if path.is_file() else []
    replaced = False
    for index, existing in enumerate(lines):
        if existing.split("=", 1)[0].strip() == key:
            lines[index] = line
            replaced = True
            break
    if not replaced:
        lines.append(line)
    path.write_text("\n".join(lines) + "\n")


def spawn_daemon():
    cache.mkdir(parents=True, exist_ok=True)
    log = cache / "daemon.log"
    with log.open("a") as handle:
        subprocess.Popen(
            [str(daemon)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=handle,
            start_new_session=True,
        )
    for _ in range(100):
        if '"running":true' in run([str(cli), "--status"], timeout=2).stdout.replace(" ", ""):
            return True
        time.sleep(0.05)
    return False

try:
    stopped_old = False
    hosts_attempted = False
    had_cli = cli.is_file()
    for name, source in sources.items():
        transaction.copy_binary(source, prefix / "bin" / name)
    write = transaction.write

    write(data / "dbus-1/services/org.quicklook.Preview.service", f"[D-BUS Service]\nName=org.quicklook.Preview\nExec={json.dumps(str(daemon),ensure_ascii=False)}\n")
    write(data / "applications/org.quicklook.Preview.desktop", f"[Desktop Entry]\nType=Application\nName=Quick Look\nComment=Native Wayland file previews\nExec={desktop_arg(cli)} --show %U\nIcon=org.quicklook.Preview\nTerminal=false\nCategories=Utility;Viewer;\nStartupNotify=false\n")
    write(data / "applications/org.quicklook.Preview.Settings.desktop", f"[Desktop Entry]\nType=Application\nName=Quick Look Settings\nName[zh_CN]=Quick Look 设置\nExec={desktop_arg(cli)} --settings\nIcon=org.quicklook.Preview\nTerminal=false\nCategories=Settings;Utility;\n")
    # 自启条目不在这里拼：它和窗口规则一样由 hosts.rs 维护（设置窗口、--apply-hosts
    # 与 --apply-window-rules 共用同一份实现），这里只把用户的选择写进配置项。
    # 配置项是唯一真相：--no-autostart 也要写下去，否则会出现「配置说开、磁盘无
    # 条目」的不一致状态，之后任意一次 --apply-hosts 又会把它悄悄建回来。
    set_config_option(
        config / "quicklook/config.toml", "autostart", "false" if args.no_autostart else "true"
    )
    icons = ROOT / "integration/icons"
    for source in sorted((icons / "hicolor").rglob("*.png")):
        relative = source.relative_to(icons / "hicolor")
        transaction.write_bytes(data / "icons/hicolor" / relative, source.read_bytes())
    transaction.write_bytes(data / "icons/hicolor/scalable/apps/org.quicklook.Preview.svg", (icons / "org.quicklook.Preview.svg").read_bytes())
    # 桌面环境按名字查图标走的是 icon-theme.cache；不重建的话新装的图标查不到，
    # 托盘与桌面条目都会退回宿主兜底图形。
    if not args.dry_run:
        run(["gtk-update-icon-cache", "-f", "-t", "-q", str(data / "icons/hicolor")])

    write(prefix / "bin/quicklook-pick", """#!/usr/bin/env bash
set -euo pipefail
selection=$(mktemp)
trap 'rm -f -- "$selection"' EXIT
fzf --read0 --print0 --multi --prompt='预览文件 > ' < <(find "${1:-$HOME}" -maxdepth 6 -type d \\( -name .git -o -name target -o -name node_modules -o -name .cache \\) -prune -o -type f -print0 2>/dev/null) > "$selection" || exit 0
[[ -s "$selection" ]] || exit 0
"$(dirname -- "$0")/quicklook" --stdin0 < "$selection"
""", executable=True)

    # 适配器必须先于 --apply-hosts 落盘：hosts.rs 只有在库已存在时才会写
    # Nemo/Thunar/Caja 的 LD_PRELOAD 启动器与 desktop 覆盖。
    for name in ("libquicklook_nemo.so", "libquicklook_thunar.so", "libquicklook_caja.so"):
        adapter = ROOT / "target" / profile / name
        if not adapter.is_file():
            print(f"{name} 缺失：对应的空格接管不可用（右键预览仍可用）。")
            continue
        transaction.copy_binary(adapter, prefix / "lib/quicklook" / name)

    if args.with_niri:
        niri_config = config / "niri/config.kdl"
        rules_file = config / "niri/quicklook.kdl"
        original = niri_config.read_text()
        rules = (ROOT / "integration/niri.kdl").read_text()
        if "Ctrl+Alt+P" not in original:
            rules += f'\nbinds {{\n    Ctrl+Alt+P hotkey-overlay-title="文件预览" {{ spawn {json.dumps(str(cli),ensure_ascii=False)} "--pick"; }}\n}}\n'
        has_include = any(
            line.strip().startswith("include") and "quicklook.kdl" in line
            for line in original.splitlines()
        )
        proposed = original if has_include else original.rstrip() + "\n\n// Quick Look file preview\n" + 'include "quicklook.kdl"\n'
        # Validate through the real include mechanism (inlining a `binds` block into
        # a config that already has one is rejected, even though separate includes
        # are fine). Nothing is written until this passes.
        if shutil.which("niri") and not args.dry_run:
            with tempfile.NamedTemporaryFile(mode="w", suffix=".kdl", prefix=".quicklook-rules-", dir=niri_config.parent, delete=False) as handle:
                handle.write(rules)
                check_rules = Path(handle.name)
            check_main = niri_config.with_name(f".quicklook-check-{stamp}.kdl")
            check_main.write_text(replace_include(proposed, check_rules.name))
            validation = run(["niri", "validate", "--config", str(check_main)])
            check_rules.unlink(missing_ok=True)
            check_main.unlink(missing_ok=True)
            if validation.returncode != 0:
                raise RuntimeError(f"niri validate 失败：{validation.stderr.strip() or validation.stdout.strip()}")
        write(rules_file, rules)
        write(niri_config, proposed)
        # niri-stack's Mod+E helper directly starts `thunar`, bypassing desktop
        # overrides. Migrate only the known command; preserve its Wayland/input
        # method environment and fall back when the user disables integration.
        helper = prefix / "bin/launch-file-manager"
        if helper.is_file() and not helper.is_symlink():
            migrated = niri_launcher.integrate(
                helper.read_text(), data / "quicklook/launchers/quicklook-thunar")
            if migrated is not None:
                write(helper, migrated, executable=True)
            else:
                print("文件管理器启动脚本有自定义内容，已保留；自定义快捷键请使用 quicklook --launch-host thunar。")

    if args.dry_run:
        print("预演完成，未改动任何文件。")
        raise SystemExit(0)

    # Post-install file verification before starting anything: a mismatch here
    # means the copy was incomplete, and rollback is still cheap.
    for name, source in sources.items():
        target = prefix / "bin" / name
        if not target.is_file() or not os.access(target, os.X_OK):
            raise RuntimeError(f"安装后校验失败：{target} 不存在或不可执行")
        if not filecmp.cmp(source, target, shallow=False):
            raise RuntimeError(f"安装后校验失败：{target} 与构建产物不一致")
    for path in (data / "dbus-1/services/org.quicklook.Preview.service", data / "applications/org.quicklook.Preview.desktop"):
        if not path.is_file():
            raise RuntimeError(f"安装后校验失败：{path} 不存在")
    # 窄入口：只读写窗口规则与自启条目，不碰文件管理器。
    applied = run([str(cli), "--apply-window-rules"], timeout=20)
    if applied.returncode != 0:
        raise RuntimeError(
            f"安装后校验失败：--apply-window-rules 返回 {applied.returncode}\n{applied.stdout}{applied.stderr}"
        )
    entry = config / "autostart/org.quicklook.Preview.desktop"
    if not args.no_autostart and not entry.is_file():
        raise RuntimeError("安装后校验失败：autostart 条目不存在")
    if args.no_autostart and entry.is_file():
        raise RuntimeError("安装后校验失败：--no-autostart 之后 autostart 条目仍然存在")

    # 文件管理器集成的唯一实现是 hosts.rs（设置窗口与 --apply-hosts 共用）。安装器不再
    # 自己写一遍，否则两份逻辑会漂移——之前就出现过这里还写着 system-search 的情况。
    # 放在二进制与 niri 校验之后：此前的任何失败都不会碰用户原有的集成。
    if args.with_file_managers:
        hosts_attempted = True
        applied = run([str(cli), "--apply-hosts"], timeout=30)
        output = (applied.stdout + applied.stderr).strip()
        if output:
            print(output)
        if applied.returncode != 0:
            raise RuntimeError("quicklook --apply-hosts 失败")
        for line in applied.stdout.splitlines():
            if line.startswith("写入 "):
                transaction.record["files"].append(line[len("写入 "):])

    # Stop the old broker before replacing the running code. It owns the tray and
    # may otherwise keep an old viewer/daemon pair alive until the idle timeout.
    if bus_available():
        current = run([str(cli), "--status"], timeout=3)
        if '"running":true' in current.stdout.replace(" ", ""):
            run([str(cli), "--quit"], timeout=5)
            stopped_old = True
            for _ in range(40):
                if '"running":true' not in run([str(cli), "--status"], timeout=3).stdout.replace(" ", ""):
                    break
                time.sleep(0.05)
    if not args.no_start and bus_available():
        if not spawn_daemon():
            raise RuntimeError(f"预览服务启动超时；查看 {cache / 'daemon.log'}")
        transaction.record["daemon_started"] = True
    else:
        transaction.record["daemon_started"] = False

    activation = shutil.which("dbus-update-activation-environment")
    if activation:
        keys = [key for key in ("WAYLAND_DISPLAY", "DISPLAY", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE") if key in os.environ]
        run([activation, "--systemd", *keys])
except Exception:
    if transaction.record.get("daemon_started"):
        run([str(cli), "--quit"], timeout=5)
    rolled_back = False
    if hosts_attempted:
        # hosts.rs 没有逐文件备份：把集成恢复成与回滚后的二进制配套的状态。
        if not had_cli:
            # 这个前缀原本没有 quicklook，本次写下的集成就是全部，直接移除。
            run([str(cli), "--remove-hosts"], timeout=30)
        # 先回滚二进制：旧 CLI 会按自己的版本重写集成，而它可能还没有 --remove-hosts。
        transaction.rollback()
        rolled_back = True
        if had_cli and cli.is_file():
            restored = run([str(cli), "--apply-hosts"], timeout=30)
            if restored.returncode != 0:
                print("回滚时恢复文件管理器集成失败：\n  " + (restored.stdout + restored.stderr).strip())
    if not rolled_back:
        transaction.rollback()
    # If this install replaced a running broker, bring the restored version back.
    if stopped_old and not args.no_start and bus_available() and cli.is_file():
        spawn_daemon()
    raise

log_dir = ROOT / ".local/dev-environment"
log_dir.mkdir(parents=True, exist_ok=True)
record = {"time": stamp, "profile": profile, "files": transaction.record["files"], "backups": transaction.record["backups"]}
record.update(file_managers=args.with_file_managers, niri=args.with_niri, autostart=not args.no_autostart)
record["version"] = run([str(cli), "--version"]).stdout.strip().removeprefix("quicklook ")
record.update(daemon_started=transaction.record.get("daemon_started", False))
log = log_dir / f"local-install-{stamp}.json"
log.write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
print(f"Installed: {cli}\nRecord: {log}")
