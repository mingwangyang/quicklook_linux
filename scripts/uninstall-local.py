#!/usr/bin/env python3
"""Remove a local Quick Look install without touching anything the user wrote.

The installer only writes app-owned files plus marked blocks inside user files.
This script mirrors that contract: app-owned paths are deleted, managed blocks
are removed through `quicklook --remove-hosts`, and the niri include is taken
back out of the user's config instead of deleting the config file.
"""
import argparse
import datetime
import glob
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time
import niri_launcher

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description="卸载本机 Quick Look 安装")
parser.add_argument("--purge", action="store_true", help="同时删除配置与缓存")
parser.add_argument("--dry-run", action="store_true", help="只显示会删除的内容")
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


def guard(path):
    """拒绝把家目录、根目录之类当成安装前缀删掉。"""
    resolved = path.expanduser().resolve()
    if not resolved.is_absolute() or resolved in (Path("/"), Path.home().resolve()):
        raise SystemExit(f"拒绝操作不安全的路径：{resolved}")
    return resolved


prefix = guard(prefix)
data = guard(data)
config = guard(config)
cli = prefix / "bin/quicklook"
removed, missing, failed, notes = [], [], [], []
record = {"time": stamp, "purge": args.purge, "dry_run": args.dry_run}


def unlink(path):
    try:
        if path.is_symlink() or path.is_file():
            if not args.dry_run:
                path.unlink()
            removed.append(str(path))
        else:
            missing.append(str(path))
    except OSError as error:
        failed.append({"path": str(path), "error": str(error)})


def run(command, timeout=10):
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        return subprocess.CompletedProcess(command, 127, "", str(error))


def status():
    result = run([str(cli), "--status"], timeout=3)
    try:
        return json.loads(result.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {}


def stop_service():
    """Stop the broker (and normally the viewer) before deleting the binaries.

    The daemon is the only process holding the tray icon. Deleting files under a
    running process is not fatal on Linux, but leaving the tray and old code
    alive after an uninstall is exactly the kind of ambiguity this script exists
    to avoid.
    """
    if not cli.is_file():
        return
    before = status()
    if not before.get("running"):
        return
    if args.dry_run:
        notes.append("会停止正在运行的预览服务")
        return
    run([str(cli), "--quit"], timeout=5)
    for _ in range(40):
        if not status().get("running"):
            notes.append("已停止预览服务")
            return
        time.sleep(0.05)
    pid = status().get("service_pid")
    if isinstance(pid, int):
        try:
            executable = Path(f"/proc/{pid}/exe").resolve()
            if executable.name == "quicklookd" and str(executable).startswith(str(prefix)):
                os.kill(pid, signal.SIGTERM)
                notes.append(f"预览服务未响应 --quit，已发送 SIGTERM（pid {pid}）")
                time.sleep(0.5)
        except (OSError, ValueError):
            pass


def remove_niri_include():
    niri_config = config / "niri/config.kdl"
    rules = config / "niri/quicklook.kdl"
    include = 'include "quicklook.kdl"'
    if not niri_config.is_file():
        unlink(rules)
        return
    original = niri_config.read_text()
    if include not in original:
        unlink(rules)
        return
    lines = original.splitlines()
    kept = []
    index = 0
    while index < len(lines):
        if lines[index].strip() == include:
            if kept and kept[-1].strip() == "// Quick Look file preview":
                kept.pop()
            index += 1
            continue
        kept.append(lines[index])
        index += 1
    proposed = "\n".join(kept).rstrip() + "\n"
    if args.dry_run:
        notes.append(f"会从 {niri_config} 移除 {include}")
        unlink(rules)
        return
    # Validate the edited config before replacing it, so a broken edit can never
    # leave niri unable to start. The rules file is only deleted once the config
    # no longer references it.
    temporary = niri_config.with_name(niri_config.name + ".quicklook-uninstalling")
    temporary.write_text(proposed)
    validation = run(["niri", "validate", "--config", str(temporary)])
    if validation.returncode != 0 and shutil.which("niri"):
        temporary.unlink(missing_ok=True)
        failed.append({"path": str(niri_config), "error": "编辑后的配置未通过 niri validate，未改动"})
        notes.append("保留 quicklook.kdl 与 include，避免留下无法启动的 niri 配置")
        return
    backup = niri_config.with_name(niri_config.name + ".quicklook-backup-" + stamp)
    shutil.copy2(niri_config, backup)
    temporary.replace(niri_config)
    removed.append(f"{niri_config}（移除 include）")
    unlink(rules)


def remove_hosts():
    if not cli.is_file():
        notes.append("找不到已安装的 quicklook，跳过文件管理器集成清理；请检查宿主菜单")
        return
    command = [str(cli), "--remove-hosts"]
    if args.dry_run:
        command.append("--dry-run")
    result = run(command, timeout=30)
    output = (result.stdout + result.stderr).strip()
    if result.returncode != 0:
        failed.append({"path": "host integrations", "error": output or "quicklook --remove-hosts 失败"})
        return
    if output:
        notes.extend(output.splitlines())


def restore_file_manager_launcher():
    helper = prefix / "bin/launch-file-manager"
    if not helper.is_file() or helper.is_symlink():
        return
    try:
        original = helper.read_text()
        proposed = niri_launcher.remove(original)
        if proposed == original:
            return
        if args.dry_run:
            notes.append(f"会从 {helper} 移除 Quick Look 启动块")
            return
        shutil.copy2(helper, helper.with_name(helper.name + ".quicklook-backup-" + stamp))
        temporary = helper.with_name(helper.name + ".quicklook-uninstalling")
        temporary.write_text(proposed)
        shutil.copymode(helper, temporary)
        temporary.replace(helper)
        notes.append(f"已恢复 {helper} 的原有启动命令")
    except (OSError, ValueError) as error:
        failed.append({"path": str(helper), "error": str(error)})


stop_service()
remove_niri_include()
remove_hosts()
restore_file_manager_launcher()

app_files = [
    prefix / "bin" / name
    for name in (
        "quicklook",
        "quicklookd",
        "quicklook-viewer",
        "quicklook-settings",
        "quicklook-nautilus-bridge",
        "quicklook-clipboard",
        "quicklook-pick",
        "quicklook-nemo",
        "quicklook-thunar",
        "quicklook-caja",
    )
]
app_files += [
    prefix / "lib/quicklook/libquicklook_nemo.so",
    prefix / "lib/quicklook/libquicklook_thunar.so",
    prefix / "lib/quicklook/libquicklook_caja.so",
    data / "applications/org.quicklook.Preview.desktop",
    data / "applications/org.quicklook.Preview.Settings.desktop",
    data / "dbus-1/services/org.quicklook.Preview.service",
    data / "nemo/actions/quicklook.nemo_action",
    data / "kio/servicemenus/quicklook.desktop",
    data / "nautilus/scripts/Quick Look",
    data / "caja-python/extensions/quicklook.py",
    *(data / "quicklook/launchers" / ("quicklook-" + host) for host in ("nemo", "caja", "thunar")),
    config / "autostart/org.quicklook.Preview.desktop",
]
# 宿主的 desktop 条目是按系统条目复制出来再改 Exec 的，用户自己也可能写过同名文件。
# 只有内容里确实带着我们的启动器才认作本应用所有，否则会把用户自己的覆盖删掉。
for name, marker in (
    ("nemo", "quicklook-nemo"),
    ("thunar", "quicklook-thunar"),
    ("caja", "quicklook-caja"),
    ("caja-browser", "quicklook-caja"),
    ("caja-folder-handler", "quicklook-caja"),
    ("caja-home", "quicklook-caja"),
    ("caja-computer", "quicklook-caja"),
):
    entry = data / f"applications/{name}.desktop"
    if not entry.is_file():
        continue
    try:
        owned = marker in entry.read_text()
    except OSError as error:
        failed.append({"path": str(entry), "error": str(error)})
        continue
    if owned:
        app_files.append(entry)
    else:
        notes.append(f"保留不属于 Quick Look 的 {entry}")
# Nautilus 的原生预览服务可能属于桌面环境；只删确实指向我们的桥的那份。
nautilus_service = data / "dbus-1/services/org.gnome.NautilusPreviewer.service"
if nautilus_service.is_file():
    try:
        if "quicklook-nautilus-bridge" in nautilus_service.read_text():
            app_files.append(nautilus_service)
        else:
            notes.append(f"保留不属于 Quick Look 的 {nautilus_service}")
    except OSError as error:
        failed.append({"path": str(nautilus_service), "error": str(error)})

icon_root = data / "icons/hicolor"
if icon_root.is_dir():
    app_files += [Path(path) for path in glob.glob(str(icon_root / "**/apps/org.quicklook.Preview.*"), recursive=True)]

for path in app_files:
    unlink(path)

# Only empty directories that this install owns. `rmdir` never touches content.
for directory in (
    prefix / "lib/quicklook",
    prefix / "lib",
    prefix / "bin",
    data / "caja-python/extensions",
):
    try:
        if directory.is_dir() and not any(directory.iterdir()) and not args.dry_run:
            directory.rmdir()
    except OSError:
        pass

if icon_root.is_dir() and shutil.which("gtk-update-icon-cache"):
    if not args.dry_run:
        run(["gtk-update-icon-cache", "-f", "-t", "-q", str(icon_root)])

if args.purge:
    for directory in (config / "quicklook", cache):
        if directory.is_dir():
            if args.dry_run:
                notes.append(f"会删除 {directory}")
            else:
                shutil.rmtree(directory, ignore_errors=True)
                removed.append(str(directory))

# Verify the app-owned paths are gone. A failed removal (permissions, immutable
# files, a running helper) must be visible instead of silently "successful".
leftovers = [
    path
    for path in app_files
    if not args.dry_run and (path.exists() or path.is_symlink())
]
if leftovers:
    failed.extend({"path": str(path), "error": "仍然存在"} for path in leftovers)

record.update(removed=removed, missing=missing, failed=failed, notes=notes)
log_dir = ROOT / ".local/dev-environment"
if not args.dry_run:
    log_dir.mkdir(parents=True, exist_ok=True)
    (log_dir / f"local-uninstall-{stamp}.json").write_text(
        json.dumps(record, ensure_ascii=False, indent=2) + "\n"
    )

print(("预演（未删除任何文件）" if args.dry_run else "卸载完成"))
print(f"删除 {len(removed)} 项，未安装 {len(missing)} 项，失败 {len(failed)} 项")
for line in notes:
    print(line)
for item in failed:
    print(f"失败：{item['path']}：{item['error']}")
if not args.dry_run:
    print(f"记录：{log_dir / f'local-uninstall-{stamp}.json'}")
raise SystemExit(1 if failed else 0)
