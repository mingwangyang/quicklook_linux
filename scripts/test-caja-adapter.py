#!/usr/bin/env python3
"""Validate the Caja adapter against a real Caja inside Xephyr + a private D-Bus bus.

用户的 Caja、真实会话总线和 Wayland 输入都不会被碰到：整套测试跑在一次性 X 显示
和临时 XDG 目录里，按键只发给那个隔离的 Xephyr。

两条实测出来的规矩：
- Caja 在一条总线上是单实例（org.mate.Caja），第二次启动只是让主进程多开一个窗口，
  所以窗口按标题（目录名）认，进程号不能当标识用。
- Xephyr 里的指针位置由宿主机鼠标决定，XTEST 点不到指定图标，选中项一律用 Ctrl+A 造。
"""
import argparse, json, os, shutil, signal, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--profile', default='debug', choices=['debug', 'release'])
parser.add_argument('--loader', default='extension', choices=['extension', 'preload'])
args = parser.parse_args()

BIN = ROOT / 'target' / args.profile
MODULE = BIN / 'libquicklook_caja.so'
FIXTURES = ROOT / '.local' / 'smoke' / 'files'
for name in ('quicklook', 'quicklookd', 'quicklook-viewer', 'libquicklook_caja.so'):
    assert (BIN / name).is_file(), f'缺少 {BIN / name}；先跑 cargo build --workspace'
assert FIXTURES.is_dir(), '先运行 scripts/smoke-test.py 生成测试文件'

OUT = ROOT / '.local' / 'caja-adapter-test'
if OUT.exists():
    shutil.rmtree(OUT)
HOME_DIR, CONFIG, DATA = OUT / 'home', OUT / 'config', OUT / 'data'
CACHE, LOGS = OUT / 'cache', OUT / 'logs'
SINGLE, MULTI, EMPTY = OUT / 'qlsingle', OUT / 'qlmulti', OUT / 'qlempty'
for directory in (HOME_DIR, CONFIG, DATA, CACHE, LOGS, SINGLE, MULTI, EMPTY, OUT / 'state'):
    directory.mkdir(parents=True)
shutil.copy(FIXTURES / '01-image.png', SINGLE / '01-image.png')
for name in ('01-image.png', '02-中文.txt', '05-data.json'):
    shutil.copy(FIXTURES / name, MULTI / name)

DISPLAY = next(f':{n}' for n in range(99, 130) if not Path(f'/tmp/.X11-unix/X{n}').exists())
ADDRESS = OUT / 'address'
ADAPTER_LOG = CACHE / 'quicklook' / 'caja-adapter.log'
MENU_MARKER = LOGS / 'menu-hits.jsonl'
CAJA_VERSION = subprocess.run(['/usr/bin/caja', '--version'], capture_output=True, text=True).stdout.strip()
ENV = {}


def base_env(**extra):
    """隔离环境：抹掉真实会话的痕迹，测试窗口只会出现在 Xephyr 里。"""
    drop = {'DBUS_SESSION_BUS_ADDRESS', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'GDK_BACKEND',
            'LD_PRELOAD', 'QUICKLOOK_DBUS_ADDRESS', 'QUICKLOOK_ADAPTER_TEST'}
    env = {k: v for k, v in os.environ.items() if k not in drop}
    env.update(HOME=str(HOME_DIR), XDG_CONFIG_HOME=str(CONFIG), XDG_DATA_HOME=str(DATA),
               XDG_CACHE_HOME=str(CACHE), XDG_STATE_HOME=str(OUT / 'state'), DISPLAY=DISPLAY,
               NO_AT_BRIDGE='1', GTK_A11Y='none', GIO_USE_VFS='local', GIO_USE_VOLUME_MONITOR='unix')
    env.update(extra)
    return env


def bus_env(**extra):
    return base_env(DBUS_SESSION_BUS_ADDRESS=ADDRESS.read_text(), **extra)


def run(cmd, env=None, timeout=25):
    return subprocess.run([str(c) for c in cmd], env=env or ENV, capture_output=True, text=True, timeout=timeout)


def sh(cmd, env=None, timeout=25):
    return run(cmd, env, timeout).stdout.strip()


def probe(condition, timeout=6):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            if condition():
                return True
        except Exception:
            pass
        time.sleep(0.1)
    return False


def wait(condition, timeout=30, why=''):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = condition()
            if value:
                return value
        except Exception:
            pass
        time.sleep(0.1)
    raise AssertionError(f'超时 {timeout}s：{why}')


def status():
    return json.loads(sh([BIN / 'quicklook', '--status']))


def showing(name):
    value = status()
    return value.get('visible') and value.get('file', {}).get('name') == name


def hidden():
    return not status().get('visible')


def adapter_text():
    return ADAPTER_LOG.read_text() if ADAPTER_LOG.exists() else ''


def activations():
    """适配器真正发起预览的次数，用来数「一次按下切了几次」。"""
    return adapter_text().count(' 预览 ')


def caja_logs():
    return ''.join(item.read_text() for item in LOGS.glob('caja-*.log'))


def fail(message):
    try:
        sh(['import', '-window', 'root', str(LOGS / 'failure.png')], timeout=10)
    except Exception:
        pass
    print('--- 适配器日志 ---')
    print(adapter_text()[-4000:] or '(空)')
    print('--- Caja stderr（去掉 GVfs 噪声）---')
    print('\n'.join(line for line in caja_logs().splitlines()
                    if 'GFileInfo created without' not in line and 'gfileinfo.c' not in line)[-3000:])
    print('--- 测试 broker ---')
    print((LOGS / 'broker.log').read_text()[-1200:] if (LOGS / 'broker.log').exists() else '(空)')
    raise AssertionError(message)


def caja_pids():
    """只认挂在测试私有总线上的 Caja：用户自己开的 Caja 进程一律不碰。"""
    address = ADDRESS.read_text() if ADDRESS.exists() else ''
    if not address:
        return []
    keep = []
    for pid in sh(['pgrep', '-x', 'caja']).split():
        try:
            environ = Path(f'/proc/{pid}/environ').read_bytes().decode(errors='ignore')
        except OSError:
            continue
        if address in environ:
            keep.append(pid)
    return keep


class Window:
    """Caja 的一个浏览窗口；按键前先重新聚焦，Xephyr 里 marco 的焦点跟不上点击。"""

    def __init__(self, folder, inject=True):
        self.name = folder.name
        log = (LOGS / f'caja-{self.name}.log').open('a')
        extra = {'QUICKLOOK_CAJA_DEBUG': '1'} if inject else {}
        if inject and args.loader == 'preload':
            extra['LD_PRELOAD'] = str(MODULE)
        # 菜单项最终执行的是 quicklook，它得能看见测试 broker，所以 Caja 挂在私有总线上。
        self.launcher = subprocess.Popen(['/usr/bin/caja', '--no-desktop', str(folder)],
                                         env=bus_env(**extra), stdout=log, stderr=log,
                                         start_new_session=True)
        log.close()
        self.wid = wait(self.find, 30, f'{self.name} 窗口没出现')
        self.pid = int(sh(['xdotool', 'getwindowpid', self.wid]))

    def find(self):
        """Caja 还挂着一个 10x10 的服务窗口，标题不是目录名，按标题加面积筛真正的浏览窗。"""
        areas = {}
        for wid in dict.fromkeys(sh(['xdotool', 'search', '--all', '--name', self.name]).split()):
            try:
                shell = sh(['xdotool', 'getwindowgeometry', '--shell', wid])
                fields = dict(line.split('=', 1) for line in shell.splitlines() if '=' in line)
                area = int(fields['WIDTH']) * int(fields['HEIGHT'])
            except Exception:
                continue
            if area > 10_000:
                areas[wid] = area
        return max(areas, key=areas.get) if areas else None

    def focus(self):
        sh(['xdotool', 'windowfocus', '--sync', self.wid])
        sh(['xdotool', 'windowactivate', '--sync', self.wid])

    def press(self, key, times=1):
        self.focus()
        for _ in range(times):
            sh(['xdotool', 'key', '--clearmodifiers', key])
            time.sleep(0.15)

    def key_down(self, key):
        sh(['xdotool', 'keydown', '--clearmodifiers', key])

    def key_up(self, key):
        sh(['xdotool', 'keyup', '--clearmodifiers', key])

    def burst(self, key, repeats=6, gap=0.05):
        """只发按下不抬：模拟长按期间的自动重复。XTEST 造不出 GDK 的重复标记，
        所以这里走的是同一套状态机的时间锁存分支。"""
        self.focus()
        self.key_down(key)
        for _ in range(repeats):
            time.sleep(gap)
            self.key_down(key)

    def select_all(self):
        self.press('ctrl+a')
        time.sleep(0.3)

    def close(self):
        self.press('ctrl+w')
        probe(lambda: self.find() is None, 10)

    def rss_kb(self):
        for line in Path(f'/proc/{self.pid}/status').read_text().splitlines():
            if line.startswith('VmRSS:'):
                return int(line.split()[1])
        raise AssertionError('读不到 Caja 的 RSS')


def quit_caja():
    """单实例：--quit 关的是主进程，等它真的消失再起下一个，否则新进程只会转发给旧的。"""
    run(['/usr/bin/caja', '--quit'], timeout=10)
    if not probe(lambda: not caja_pids(), 20):
        for pid in caja_pids():
            subprocess.run(['kill', '-9', pid], capture_output=True)
        probe(lambda: not caja_pids(), 10)
    time.sleep(0.5)


def open_with(window, name, key='space'):
    window.press(key)
    if not probe(lambda: showing(name), 15):
        fail(f'按 {key} 没有打开 {name}；当前状态 {json.dumps(status(), ensure_ascii=False)}')


def close_with(window, key='space'):
    window.press(key)
    if not probe(hidden, 12):
        fail(f'再按 {key} 没有关闭预览；当前状态 {json.dumps(status(), ensure_ascii=False)}')


def select_and_open(window, count, key='space'):
    """目录模型是异步加载的：Ctrl+A 按早了什么都选不中，空格于是空转。
    用适配器自己报的选中项数量来确认，没中就重新聚焦再按一轮；成功那一轮的
    空格已经把预览打开，调用方接着验内容、再关闭。"""
    for _ in range(8):
        ADAPTER_LOG.unlink(missing_ok=True)
        window.select_all()
        time.sleep(0.45)
        window.press(key)
        if probe(lambda: f'选中 {count} 项' in adapter_text(), 3):
            return
        time.sleep(0.8)
    fail(f'Ctrl+A 一直选不中 {count} 项；适配器日志尾部：{adapter_text()[-800:]}')



# ---------- 隔离显示、私有总线和一个窗口管理器 ----------
xephyr_log = (LOGS / 'xephyr.log').open('w')
xephyr = subprocess.Popen(['Xephyr', DISPLAY, '-screen', '1280x800x24', '-ac', '-nocursor'],
                          stdout=xephyr_log, stderr=xephyr_log)
wait(lambda: Path(f'/tmp/.X11-unix/X{DISPLAY[1:]}').exists(), 15, 'Xephyr 没起来')

holder = f'printf %s "$DBUS_SESSION_BUS_ADDRESS" > "{ADDRESS}"; "{BIN}/quicklookd" >> "{LOGS}/broker.log" 2>&1 & sleep 900'
session_log = (LOGS / 'session.log').open('w')
session = subprocess.Popen(['dbus-run-session', '--', 'bash', '-c', holder],
                           env=base_env(), stdout=session_log, stderr=session_log, start_new_session=True)
wait(ADDRESS.exists, 20, '私有总线地址没写出来')
ENV = bus_env()
wait(lambda: status().get('running'), 20, '测试 broker 没起来')
# 没有窗口管理器时 GTK 收不到 focus-in，窗口内部没有焦点控件，Ctrl+A 也不会选中文件。
marco = subprocess.Popen(['marco'], env=ENV, stdout=(LOGS / 'marco.log').open('w'),
                         stderr=subprocess.STDOUT, start_new_session=True)
time.sleep(1.5)

# 右键菜单用一份带记录的副本：只多打一行日志，判定逻辑与安装的那份一致。
menu_text = (
    (ROOT / 'integration/caja/quicklook.py.in').read_text()
    .replace('@QUICKLOOK_CLI@', json.dumps(str(BIN / 'quicklook')))
    .replace('@QUICKLOOK_ADAPTER@', json.dumps(str(MODULE)))
    .replace('@LOAD_ADAPTER@', 'True' if args.loader == 'extension' else 'False')
)
menu_dir = DATA / 'caja-python/extensions'
menu_dir.mkdir(parents=True, exist_ok=True)
(menu_dir / 'quicklook.py').write_text(menu_text + f'''
_asked = QuickLookProvider.get_file_items
def _recorded(self, window, files):
    import json as _json
    items = _asked(self, window, files)
    with open({str(MENU_MARKER)!r}, 'a') as handle:
        handle.write(_json.dumps({{'uris': [file.get_uri() for file in files],
                                  'labels': [item.get_property('label') for item in items]}}) + chr(10))
    return items
QuickLookProvider.get_file_items = _recorded
''')

(CONFIG / 'quicklook').mkdir(parents=True, exist_ok=True)
(CONFIG / 'quicklook/config.toml').write_text('hijack_native = true\n')
results = {'caja': CAJA_VERSION, 'display': DISPLAY, 'isolated_bus': True, 'loader': args.loader}
windows = []
try:
    # ---------- 阶段一：默认空格 ----------
    single = Window(SINGLE)
    windows.append(single)
    wait(lambda: '按键旁路器已安装' in adapter_text(), 20, '按键旁路器没装上')
    results['snooper_installed'] = True
    if args.loader == 'extension':
        assert b'LD_PRELOAD=' not in Path(f'/proc/{single.pid}/environ').read_bytes()
        results['plain_caja_without_preload'] = True

    select_and_open(single, 1)
    if not probe(lambda: showing('01-image.png'), 15):
        fail(f'空格没有打开选中项的预览：{json.dumps(status(), ensure_ascii=False)}')
    results['space_opens_selection'] = True
    close_with(single)

    # 只按下不抬起的一串事件（长按的形状）只能切换一次。
    ADAPTER_LOG.unlink(missing_ok=True)
    single.burst('space')
    if not probe(lambda: status().get('visible'), 12):
        fail('连发按下（模拟长按）没有打开预览')
    single.key_up('space')
    counts = activations()
    assert counts == 1, f'模拟长按期间切换了 {counts} 次，应当只有第一次生效'
    results['repeat_presses_open_once'] = counts
    if not hidden():
        close_with(single)

    # 真·长按：不自己补发按下，交给 X 服务器的自动重复。适配器看到的 kind=8 事件数
    # 就是这段时间锁存要处理的重复节奏，也是「一次长按只开一次」的实测证据。
    time.sleep(0.6)
    ADAPTER_LOG.unlink(missing_ok=True)
    single.focus()
    single.key_down('space')
    time.sleep(1.2)
    assert status().get('visible'), '长按期间预览不能反复开关'
    single.key_up('space')
    hold = {'press_events': adapter_text().count('kind=8 keyval=0x20'),
            'activations': activations()}
    results['real_hold'] = hold
    assert hold['activations'] == 1, f'长按切换了 {hold["activations"]} 次，一次长按只能开一次：{hold}'
    # The viewer closes on a Space release paired with a press it received.
    # If focus moved to it during a hold, releasing can close once; repeats must
    # never bounce the window back to the host and reopen it.
    if not hidden():
        close_with(single)

    # 抬起可能不回宿主。重新获得焦点立即清理旧状态，不靠等待某个时间阈值。
    time.sleep(0.6)
    ADAPTER_LOG.unlink(missing_ok=True)
    single.focus()
    single.key_down('space')
    time.sleep(0.1)
    single.key_up('space')
    single.focus()
    single.key_down('space')
    single.key_up('space')
    counts = activations()
    log_tail = adapter_text()
    assert counts == 2, (f'重新聚焦未清理旧的按键状态，两次空格只切换了 {counts} 次；'
                         f'焦点 {sh(["xdotool", "getactivewindow"])} / 宿主 {single.wid}；'
                         f'日志尾部 {log_tail[-600:]}')
    if not probe(hidden, 12):
        fail(f'第二次空格没能关掉预览；切换次数 {counts}；日志尾部 {log_tail[-600:]}')
    results['recovers_from_lost_key_release'] = counts
    time.sleep(0.6)

    # Typing in the location bar must not preview the still-selected image.
    single.press('ctrl+l')
    ADAPTER_LOG.unlink(missing_ok=True)
    single.press('space')
    time.sleep(0.6)
    assert hidden(), '地址栏空格不应触发预览'
    assert activations() == 0, '地址栏误用了文件区选择'
    single.press('Escape')
    results['location_entry_space_untouched'] = True

    # 引用计数：反复读选中项，Caja 进程内存不该持续上涨。
    before = single.rss_kb()
    for _ in range(15):
        open_with(single, '01-image.png')
        close_with(single)
    growth = single.rss_kb() - before
    assert growth < 8192, f'15 次预览后 Caja 常驻内存涨了 {growth} KiB，选中项的引用可能没放开'
    results['rss_growth_kib'] = growth

    # 多选网格：同一个 Caja 主进程再开一个窗口，按标题认。
    multi = Window(MULTI)
    windows.append(multi)
    select_and_open(multi, 3)
    if not probe(lambda: (lambda s: s.get('grid') and s.get('selection_count') == 3)(status()), 15):
        fail(f'三次选中没有出网格：{json.dumps(status(), ensure_ascii=False)}')
    results['space_multi_selection'] = True
    close_with(multi)

    # 没选中任何东西时不能吞掉空格。
    empty = Window(EMPTY)
    windows.append(empty)
    ADAPTER_LOG.unlink(missing_ok=True)
    empty.press('space')
    time.sleep(1.2)
    assert hidden(), '选中项为空时空格被吞掉了'
    wait(lambda: '选中项为空，交回宿主' in adapter_text(), 10, '空选中没有走“交回宿主”分支')
    results['empty_selection_leaves_space_alone'] = True
    quit_caja()

    # ---------- 阶段二：改成 Ctrl+Space ----------
    (CONFIG / 'quicklook').mkdir(parents=True, exist_ok=True)
    (CONFIG / 'quicklook' / 'config.toml').write_text('hijack_native = true\npreview_key_caja = "Ctrl+Space"\n')
    ADAPTER_LOG.unlink(missing_ok=True)
    single = Window(SINGLE)
    windows.append(single)
    wait(lambda: '预览键 Ctrl+空格' in adapter_text(), 20, '改键后适配器没读到新配置')
    select_and_open(single, 1, key='ctrl+space')
    if not probe(lambda: showing('01-image.png'), 15):
        fail(f'Ctrl+空格没有打开预览：{json.dumps(status(), ensure_ascii=False)}')
    results['remapped_key_opens'] = True
    close_with(single, 'ctrl+space')
    results['remapped_key_closes'] = True
    # 选中项还在，这时按裸空格必须什么都不发生——证明改键改的是接管键，不是碰巧没选中。
    single.press('space')
    time.sleep(1.2)
    assert hidden(), '改了键，裸空格不该再唤出预览'
    results['plain_space_inert_after_remap'] = True

    # ---------- 阶段三：右键菜单 provider ----------
    ADAPTER_LOG.unlink(missing_ok=True)
    single.press('Menu')
    hits = wait(lambda: MENU_MARKER.exists() and MENU_MARKER.read_text(), 20,
                '上下文菜单没有询问扩展 provider')
    last = json.loads(hits.strip().splitlines()[-1])
    wanted = (SINGLE / '01-image.png').as_uri()
    assert last['uris'] == [wanted], f'上下文菜单问的不是当前选中项：{last}'
    assert any('Quick Look' in label for label in last['labels']), f'菜单里没有预览项：{last}'
    results['context_menu_provider_queried'] = last['labels']
    sh(['xdotool', 'key', '--clearmodifiers', 'Escape'])
    sh(['import', '-window', 'root', str(LOGS / 'context-menu.png')])
    # Activate the real extension item's GObject signal. A callback arity error must
    # fail here rather than being hidden by separately launching the CLI.
    activation = run(['python3', '-c',
        'import importlib.util; from gi.repository import Gio; '
        's=importlib.util.spec_from_file_location("qlmenu", ' + repr(str(menu_dir / 'quicklook.py')) + '); '
        'm=importlib.util.module_from_spec(s); s.loader.exec_module(m); '
        'f=type("File", (), {"get_uri": lambda self: ' + repr(wanted) + '})(); '
        'm.QuickLookProvider().get_file_items(None, [f])[0].activate()'])
    assert 'TypeError' not in activation.stderr, activation.stderr
    if not probe(lambda: showing('01-image.png'), 15):
        fail(f'菜单项要执行的 quicklook 命令没有打开预览；当前状态 {json.dumps(status(), ensure_ascii=False)}')
    results['menu_activation_opens_preview'] = True
    run([BIN / 'quicklook', '--close'])
    quit_caja()

    # ---------- 阶段四：关闭接管后普通命令不接管 ----------
    (CONFIG / 'quicklook' / 'config.toml').unlink(missing_ok=True)
    ADAPTER_LOG.unlink(missing_ok=True)
    plain = Window(SINGLE, inject=False)
    windows.append(plain)
    plain.select_all()
    plain.press('space')
    time.sleep(1.5)
    assert hidden(), '关闭接管后，空格不该被接管'
    assert not adapter_text(), '关闭接管后却安装了按键适配'
    results['disabled_takeover_space_untouched'] = True
    quit_caja()

    # ---------- 阶段五：不能对陌生类型调 Caja 的取 URI 接口 ----------
    noise = caja_logs()
    for marker in ('CAJA_IS_FILE', 'CAJA_IS_FILE_INFO', 'GLib-GObject-CRITICAL',
                   'Segmentation', 'g_return_if_fail_warning'):
        assert marker not in noise, f'Caja 日志里出现 {marker}'
    results['no_type_assertions'] = True

    (OUT / 'result.json').write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n')
    print('PASS 真实 Caja（Xephyr + marco + 私有总线）：空格预览选中项、自动重复单次切换、多选网格、'
          '空选中不吞键、改键、右键菜单 provider、关闭接管保留原行为、无类型误用')
    print(json.dumps(results, ensure_ascii=False))
finally:
    for process in (marco, session, xephyr):
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            process.terminate()
    for pid in caja_pids():
        try:
            os.kill(int(pid), signal.SIGTERM)
        except ProcessLookupError:
            pass
    for process in (marco, session, xephyr):
        try:
            process.wait(timeout=10)
        except Exception:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    print('残留 Caja 进程：', caja_pids() or '无')
    sys.stdout.flush()
