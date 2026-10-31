#!/usr/bin/env python3
"""Real Space-key checks in Xephyr and a private session bus; no user host restarts."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--profile', default='release', choices=['debug', 'release'])
parser.add_argument('--nautilus', type=Path, help='optional extracted Nautilus installation prefix')
parser.add_argument('--hosts', nargs='+', default=['nemo','caja','thunar','nautilus','dolphin','yazi'], choices=['nemo','caja','thunar','nautilus','dolphin','yazi'])
parser.add_argument('--thunar-entry', choices=['preload','launcher','desktop','dbus'], default='preload',
                    help='test a cold Thunar startup route, rather than only LD_PRELOAD')
parser.add_argument('--caja-entry', choices=['preload','extension','desktop'], default='preload',
                    help='test plain caja using its user extension, without LD_PRELOAD')
parser.add_argument('--prefix', type=Path, help='installed prefix for startup route checks')
parser.add_argument('--inner', action='store_true', help=argparse.SUPPRESS)
args = parser.parse_args()
BIN = ROOT / 'target' / args.profile
OUT = ROOT / '.local/space-key-tests'
OUT.mkdir(parents=True, exist_ok=True)
if not args.inner:
    env = dict(os.environ, HOME=str(OUT / 'home'), XDG_CONFIG_HOME=str(OUT / 'config'),
               XDG_DATA_HOME=str(OUT / 'data'), XDG_CACHE_HOME=str(OUT / 'cache'),
               XDG_STATE_HOME=str(OUT / 'state'), NO_AT_BRIDGE='1', GTK_A11Y='none',
               GIO_USE_VFS='local', GIO_USE_VOLUME_MONITOR='unix', GSETTINGS_BACKEND='keyfile')
    for key in ('WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'GDK_BACKEND', 'VIPSHOME',
                'LD_PRELOAD', 'QUICKLOOK_DBUS_ADDRESS'):
        env.pop(key, None)
    for name in ('home', 'config', 'data', 'cache', 'state'):
        (OUT / name).mkdir(exist_ok=True)
    conf = OUT / 'config/quicklook/config.toml'
    conf.parent.mkdir(exist_ok=True)
    conf.write_text('hijack_native = true\nviewer_grace_secs = 30\n')
    services = OUT / 'data/dbus-1/services'
    services.mkdir(parents=True, exist_ok=True)
    (services / 'org.gnome.NautilusPreviewer.service').write_text(
        '[D-BUS Service]\nName=org.gnome.NautilusPreviewer\nExec=' + str(BIN / 'quicklook-nautilus-bridge') + '\n')
    command = ['dbus-run-session', '--', sys.executable, __file__, '--inner', '--profile', args.profile]
    command += ['--hosts', *args.hosts]
    command += ['--thunar-entry', args.thunar_entry]
    command += ['--caja-entry', args.caja_entry]
    if args.prefix:
        command += ['--prefix', str(args.prefix.resolve())]
    if args.nautilus:
        command += ['--nautilus', str(args.nautilus.resolve())]
    raise SystemExit(subprocess.run(command, cwd=ROOT, env=env).returncode)


def run(command, env=None, check=True):
    return subprocess.run(list(map(str, command)), capture_output=True, text=True,
                          env=env, check=check, timeout=15).stdout.strip()


def cli(*arguments):
    return run([BIN / 'quicklook', *arguments])


def status():
    return json.loads(cli('--status'))


def wait(condition, timeout=10):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = condition()
            if value:
                return value
        except (subprocess.CalledProcessError, json.JSONDecodeError):
            pass
        time.sleep(.01)
    raise AssertionError('timeout; see ' + str(OUT))


display = next(f':{n}' for n in range(99, 130) if not Path(f'/tmp/.X11-unix/X{n}').exists())
xephyr = subprocess.Popen(['Xephyr', display, '-screen', '1280x900x24', '-ac', '-nocursor'],
                          stdout=(OUT / 'xephyr.log').open('w'), stderr=subprocess.STDOUT,
                          start_new_session=True)
wait(lambda: Path(f'/tmp/.X11-unix/X{display[1:]}').exists())
os.environ['DISPLAY'] = display
# The private bus was started before Xephyr. Its activation environment must use
# this test display too, or a cold D-Bus host opens on the parent's X server.
run(['dbus-update-activation-environment', 'DISPLAY'])
marco = subprocess.Popen(['marco'], stdout=(OUT / 'marco.log').open('w'),
                         stderr=subprocess.STDOUT, start_new_session=True)
broker = subprocess.Popen([BIN / 'quicklookd'], stdout=(OUT / 'broker.log').open('w'),
                          stderr=subprocess.STDOUT, start_new_session=True)
children = [broker, marco, xephyr]
results = {}


def windows(pid=None, title=None):
    command = ['xdotool', 'search', '--all']
    if pid:
        command += ['--pid', str(pid)]
    command += ['--name', title or '.']
    ids = run(command, check=False).split()
    answer = []
    for wid in dict.fromkeys(ids):
        geometry = run(['xdotool', 'getwindowgeometry', '--shell', wid], check=False)
        fields = dict(line.split('=', 1) for line in geometry.splitlines() if '=' in line)
        if int(fields.get('WIDTH', 0)) * int(fields.get('HEIGHT', 0)) > 10_000:
            answer.append(wid)
    return answer


def focus(wid):
    run(['xdotool', 'windowactivate', '--sync', wid])
    run(['xdotool', 'windowfocus', '--sync', wid])


def key(value, wid=None):
    if wid:
        focus(wid)
    run(['xdotool', 'key', '--clearmodifiers', '--delay', '3', value])


try:
    wait(lambda: status().get('running'))
    if args.prefix:
        run([args.prefix / 'bin/quicklook', '--apply-hosts'])
    elif args.thunar_entry != 'preload' or args.caja_entry != 'preload':
        assert args.prefix, '--prefix is required for installed startup routes'
    fixture = ROOT / '.local/smoke/files/01-image.png'
    assert fixture.is_file(), 'run smoke-test.py first'
    cli('--show', fixture)
    wait(lambda: status().get('ready'))
    # Direct native requests must also accept deliberate consecutive invocations.
    intervals = []
    for i in range(12):
        start = time.monotonic()
        cli(fixture)
        assert status()['visible'] == bool(i % 2), 'Toggle still suppresses rapid requests'
        intervals.append(round((time.monotonic() - start) * 1000, 2))
    cli('--close')
    results['rapid_toggle_rpc_ms'] = intervals

    for host in (name for name in args.hosts if name != 'yazi'):
        if host == 'nautilus':
            program = args.nautilus / 'usr/bin/nautilus' if args.nautilus else Path('/usr/bin/nautilus')
        else:
            program = Path('/usr/bin') / host
        if not program.is_file():
            results[host] = {'tested': False, 'reason': 'host not available'}
            continue
        folder = OUT / ('space-' + host)
        folder.mkdir(exist_ok=True)
        shutil.copyfile(fixture, folder / 'preview-test.png')
        env = dict(os.environ)
        extra = []
        if (host == 'caja' and args.caja_entry == 'preload') or (host == 'thunar' and args.thunar_entry == 'preload'):
            env['LD_PRELOAD'] = str(BIN / f'libquicklook_{host}.so')
        if host == 'nemo':
            run(['gsettings', 'set', 'org.nemo.preferences', 'default-folder-viewer', 'list-view'])
            extra = ['--no-desktop']
        if host == 'caja':
            extra = ['--no-desktop']
        if host == 'nautilus' and args.nautilus:
            env['LD_LIBRARY_PATH'] = str(args.nautilus / 'usr/lib/x86_64-linux-gnu') + ':' + str(args.nautilus / 'usr/lib/x86_64-linux-gnu/libunity')
            env['GSETTINGS_SCHEMA_DIR'] = str(args.nautilus / 'usr/share/glib-2.0/schemas')
            env['XDG_DATA_DIRS'] = str(args.nautilus / 'usr/share') + ':/usr/local/share:/usr/share'
        launch = [str(program), *extra, str(folder)]
        if host == 'caja' and args.caja_entry != 'preload':
            assert 'LD_PRELOAD' not in env
            if args.caja_entry == 'desktop':
                launch = ['gtk-launch', 'caja-browser', str(folder)]
        if host == 'thunar' and args.thunar_entry != 'preload':
            assert 'LD_PRELOAD' not in env, 'startup tests must load the adapter themselves'
            if args.thunar_entry == 'launcher':
                launch = [str(OUT / 'data/quicklook/launchers/quicklook-thunar'), str(folder)]
            elif args.thunar_entry == 'desktop':
                launch = ['gtk-launch', 'thunar', str(folder)]
            else:
                run(['gdbus', 'call', '--session', '--dest', 'org.freedesktop.DBus',
                     '--object-path', '/org/freedesktop/DBus', '--method',
                     'org.freedesktop.DBus.StartServiceByName', 'org.xfce.Thunar', '0'])
        if host == 'nautilus' and args.nautilus:
            # Tracker dlopens its parser by an absolute distro path. A read-only
            # namespace provides that path without installing anything into /usr.
            system_lib = Path('/usr/lib/x86_64-linux-gnu')
            alias = OUT / 'system-libs'
            alias.mkdir(exist_ok=True)
            overlay = OUT / 'nautilus-lib-overlay'
            overlay.mkdir(exist_ok=True)
            for library in system_lib.iterdir():
                link = overlay / library.name
                if link.is_symlink(): link.unlink()
                if not link.exists():
                    link.symlink_to(alias / library.name)
            tracker = overlay / 'tracker-3.0'
            if tracker.is_symlink(): tracker.unlink()
            tracker.symlink_to(args.nautilus / 'usr/lib/x86_64-linux-gnu/tracker-3.0')
            launch = ['bwrap', '--die-with-parent', '--ro-bind', '/', '/',
                      '--bind', str(OUT), str(OUT),
                      '--ro-bind', str(system_lib), str(alias),
                      '--ro-bind', str(overlay), str(system_lib), *launch]
        if host == 'nautilus':
            monitor = subprocess.Popen(['dbus-monitor', '--session',
                "interface='org.gnome.NautilusPreviewer2'", "interface='org.freedesktop.DBus.Peer'"],
                stdout=(OUT / 'nautilus-dbus.log').open('w'), stderr=subprocess.STDOUT,
                start_new_session=True)
            children.append(monitor)
            env['G_MESSAGES_DEBUG'] = 'nautilus-previewer'
        process = subprocess.Popen(launch, env=env,
                                   stdout=(OUT / (host + '.log')).open('w'), stderr=subprocess.STDOUT,
                                   start_new_session=True)
        children.append(process)
        wid = wait(lambda: next(iter(windows(title=folder.name)), None), 30)
        if host == 'caja' and args.caja_entry != 'preload':
            pid = int(run(['xdotool','getwindowpid',wid]))
            wait(lambda: 'libquicklook_caja.so' in Path(f'/proc/{pid}/maps').read_text())
            if args.caja_entry == 'extension':
                assert b'LD_PRELOAD=' not in Path(f'/proc/{pid}/environ').read_bytes(), 'plain Caja inherited a preload'
        if host == 'caja' and args.prefix:
            pid = int(run(['xdotool','getwindowpid',wid]))
            adapter_log = OUT / 'cache/quicklook/caja-adapter.log'
            wait(lambda: adapter_log.is_file() and f'{pid} 按键旁路器已安装' in adapter_log.read_text())
            assert adapter_log.read_text().count(f'{pid} 按键旁路器已安装') == 1, 'two loaders installed duplicate key callbacks'
        if host == 'thunar' and args.thunar_entry != 'preload':
            reply = run(['gdbus','call','--session','--dest','org.freedesktop.DBus',
                         '--object-path','/org/freedesktop/DBus','--method',
                         'org.freedesktop.DBus.GetConnectionUnixProcessID','org.xfce.Thunar'])
            pid = int(reply.split('uint32 ')[1].split(',')[0].split(')')[0])
            assert 'libquicklook_thunar.so' in Path(f'/proc/{pid}/maps').read_text()
        time.sleep(.5)  # folder models load asynchronously; unrelated to preview input
        key('ctrl+a', wid)
        cli('--close')
        if host == 'nautilus':
            # GTK4 initially focuses the path bar, so Ctrl+A need not select files.
            # Click the single fixture's icon in the fixed 1280x900 test layout.
            run(['xdotool','mousemove','--window',wid,'340','180','click','1'])
            key('ctrl+a', wid)
            run(['import', '-window', wid, OUT / 'nautilus-selection.png'])
        if host == 'dolphin':
            key('space', wid)
            time.sleep(.3)
            assert not status().get('visible'), 'Dolphin unexpectedly invoked Quick Look'
            results[host] = {'tested': True, 'space_preview': False,
                             'version': run([program, '--version']), 'reason': '23.08 lacks service menu shortcuts'}
        else:
            taps = []
            for _ in range(5):
                start = time.monotonic()
                key('space', wid)
                wait(lambda: status().get('visible'), 5)
                key('space', wid)
                wait(lambda: not status().get('visible'), 5)
                taps.append(round((time.monotonic() - start) * 1000, 2))
            # Cold opening may have delivered its release before mapping. The next
            # complete Space tap in the viewer itself must close it, without arming delays.
            key('space', wid)
            wait(lambda: status().get('visible'))
            viewer = wait(lambda: next(iter(windows(pid=status().get('viewer_pid'))), None))
            key('space', viewer)
            wait(lambda: not status().get('visible'))
            results[host] = {'tested': True, 'space_preview': True, 'five_open_close_pairs_ms': taps,
                             'next_tap_in_viewer_closes': True}
            if host == 'thunar':
                results[host]['startup_route'] = args.thunar_entry
            if host == 'caja':
                results[host]['startup_route'] = args.caja_entry
        print('PASS', host, json.dumps(results[host]), flush=True)
        if host != 'nautilus':
            run([program, '--quit' if host != 'dolphin' else '--help'], env=env, check=False)
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=8)

    if 'yazi' in args.hosts and shutil.which('yazi'):
        config = OUT / 'config/yazi'
        (config / 'plugins/quicklook.yazi').mkdir(parents=True, exist_ok=True)
        plugin = (ROOT / 'integration/yazi/quicklook.yazi/main.lua').read_text().replace('@QUICKLOOK_CLI@', str(BIN / 'quicklook'))
        (config / 'plugins/quicklook.yazi/main.lua').write_text(plugin)
        (config / 'init.lua').write_text('require("quicklook"):setup {}\n')
        (config / 'keymap.toml').write_text('[[mgr.prepend_keymap]]\non = "<Space>"\nrun = "plugin quicklook"\n')
        folder = OUT / 'space-yazi'; folder.mkdir(exist_ok=True)
        shutil.copyfile(fixture, folder / 'preview-test.png')
        env = dict(os.environ, YAZI_CONFIG_HOME=str(config), TERM='xterm-256color')
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 120, 0, 0))
        client = str(os.getpid())
        process = subprocess.Popen(['yazi', '--client-id', client, str(folder)], stdin=slave,
                                   stdout=slave, stderr=slave, env=env, start_new_session=True,
                                   preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0))
        children.append(process); os.close(slave)

        def pump(duration=.02):
            until = time.monotonic() + duration
            while time.monotonic() < until:
                if select.select([master], [], [], .01)[0]:
                    chunk = os.read(master, 65536)
                    if b'\x1b[6n' in chunk: os.write(master, b'\x1b[1;1R')
                    if b'\x1b[0c' in chunk: os.write(master, b'\x1b[?1;2c')
                    if b'\x1b[?u' in chunk: os.write(master, b'\x1b[?0u')
                    if b'\x1b]11;?' in chunk: os.write(master, b'\x1b]11;rgb:1111/1515/1b1b\x1b\\')
        pump(1)
        run(['ya', 'emit-to', client, 'reveal', str(folder / 'preview-test.png')], env=env)
        pump(.1)
        durations = []
        for _ in range(5):
            start = time.monotonic()
            os.write(master, b' ')
            wait(lambda: (pump() or status().get('visible')))
            os.write(master, b' ')
            wait(lambda: (pump() or not status().get('visible')))
            durations.append(round((time.monotonic() - start) * 1000, 2))
        results['yazi'] = {'tested': True, 'space_preview': True, 'five_open_close_pairs_ms': durations}
        print('PASS yazi', json.dumps(results['yazi']), flush=True)
        process.terminate(); process.wait(timeout=5); os.close(master)
    (OUT / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n')
finally:
    # D-Bus activation is a child of the private bus, outside Popen's group.
    if args.thunar_entry != 'preload':
        run(['/usr/bin/thunar', '--quit'], check=False)
    for process in reversed(children):
        if process.poll() is None:
            try: os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for process in reversed(children):
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired: process.kill(); process.wait()
