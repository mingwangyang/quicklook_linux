#!/usr/bin/env python3
"""Exercise deb installation/removal in a private dpkg root, without sudo."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('package', type=Path)
args = parser.parse_args()
package = args.package.resolve()
assert os.geteuid() != 0, 'Run as the desktop user, without sudo'
assert shutil.which('fakeroot'), 'Install fakeroot for private dpkg install/remove checks'


def run(command, **kwargs):
    # dpkg 与 apt 的输出跟随 locale；断言里匹配的是英文串，这里固定 C locale，
    # 免得在中文桌面环境下把成功安装误判成失败。
    env = dict(kwargs.pop('env', os.environ), LC_ALL='C', LANG='C')
    result = subprocess.run(list(map(str, command)), capture_output=True, text=True, timeout=60, env=env, **kwargs)
    assert result.returncode == 0, f'{command}\n{result.stdout}\n{result.stderr}'
    return result.stdout


def files(directory):
    return {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in directory.rglob('*') if path.is_file()}


with tempfile.TemporaryDirectory(prefix='quicklook-deb-test-') as temporary:
    base = Path(temporary)
    system = base / 'system'
    (system / 'var/lib/dpkg').mkdir(parents=True)
    (system / 'var/lib/dpkg/status').touch()
    # --force-script-chrootless：维护者脚本在宿主环境里跑（dpkg 会提示用这个开关），
    # 否则它要 chroot 进私有 root，非 root 身份直接失败。
    dpkg = ['fakeroot', 'dpkg', '--root=' + str(system), '--force-not-root', '--force-depends',
            '--force-script-chrootless']
    # Package dependencies are resolved separately by apt simulation; the empty
    # private database is deliberately isolated from the host database.
    installed = run([*dpkg, '--install', package])
    assert 'Setting up quicklook' in installed
    assert 'install ok installed' in (system / 'var/lib/dpkg/status').read_text()
    metadata = base / 'metadata'
    run(['dpkg-deb', '-e', package, metadata])
    # 0.0.1-5 起带两个维护者脚本：安装/卸载时以发起安装的用户身份补/撤用户级接入。
    for name in ('postinst', 'prerm'):
        script = metadata / name
        assert script.is_file(), f'缺少维护者脚本 {name}'
        assert os.access(script, os.X_OK), f'维护者脚本不可执行 {name}'
    assert not any((metadata / name).exists() for name in ('preinst', 'postrm'))
    for line in (metadata / 'md5sums').read_text().splitlines():
        expected, relative = line.split('  ', 1)
        assert hashlib.md5((system / relative).read_bytes()).hexdigest() == expected
    home = base / 'desktop-user $HOME %i'
    data, config = home / 'data', home / 'config'
    (config / 'quicklook').mkdir(parents=True)
    config_file = config / 'quicklook/config.toml'
    config_file.write_text('hijack_native = true\npreview_key_nemo = "Ctrl+Space"\n')
    service = data / 'dbus-1/services/org.gnome.NautilusPreviewer.service'
    service.parent.mkdir(parents=True)
    original_service = '[D-BUS Service]\nName=org.gnome.NautilusPreviewer\nExec=/usr/bin/sushi\n'
    service.write_text(original_service)
    thunar_service=data/'dbus-1/services/org.xfce.Thunar.service'
    original_thunar_service='[D-BUS Service]\nName=org.xfce.Thunar\nExec=/opt/my-thunar\n'
    thunar_service.write_text(original_thunar_service)
    thunar_unit=config/'systemd/user/thunar.service.d/quicklook.conf'
    thunar_unit.parent.mkdir(parents=True)
    original_thunar_unit='[Service]\nEnvironment=KEEP_MY_OPTION=1\n'
    thunar_unit.write_text(original_thunar_unit)
    applications = data / 'applications'
    applications.mkdir()
    original_desktop = '[Desktop Entry]\nType=Application\nName=My Caja\nExec=caja --no-desktop %U\n'
    caja = applications / 'caja-browser.desktop'
    caja.write_text(original_desktop)
    env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(data),
               XDG_CONFIG_HOME=str(config), XDG_CACHE_HOME=str(home / 'cache'),
               DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(base / 'no-session-bus'),
               NO_AT_BRIDGE='1', GTK_A11Y='none')
    env.pop('LD_PRELOAD', None)
    info = json.loads((system / 'usr/share/doc/quicklook/build-info.json').read_text())
    assert run([system / 'usr/bin/quicklook', '--version'], env=env).strip() == 'quicklook ' + info['display_version']
    before = files(system / 'usr')
    # A deb install is read-only to ordinary users. This reproduces it without root.
    directories = [path for path in (system / 'usr').rglob('*') if path.is_dir()]
    for directory in directories + [system / 'usr']:
        directory.chmod(0o555)
    cli = system / 'usr/bin/quicklook'
    dry_run = run([cli, '--apply-hosts', '--dry-run'], env=env)
    assert not (data / 'quicklook/launchers').exists()
    first = run([cli, '--apply-hosts'], env=env)
    assert files(system / 'usr') == before, 'runtime wrote into installation prefix'
    for host in ('caja', 'thunar', 'nemo'):
        launcher = data / f'quicklook/launchers/quicklook-{host}'
        assert launcher.is_file(), host
        run(['bash', '-n', launcher])
    assert 'quicklook-caja' in caja.read_text()
    assert 'quicklook-nautilus-bridge' in service.read_text()
    extension=data/'caja-python/extensions/quicklook.py'
    run(['/usr/bin/python3','-c',
         'import ctypes,py_compile,sys; '
         'm=ctypes.CDLL(sys.argv[1]); '
         'f=m.quicklook_caja_install; f.argtypes=[]; f.restype=ctypes.c_int; '
         'assert f() == 0; '
         'py_compile.compile(sys.argv[2],doraise=True)',
         system/'usr/lib/quicklook/libquicklook_caja.so',extension],env=env)
    assert 'quicklook-thunar' in thunar_service.read_text()
    assert 'quicklook-thunar' in thunar_unit.read_text()
    if shutil.which('systemd-analyze'):
        units=base/'unit-check';units.mkdir()
        unit=units/'quicklook-thunar-check.service'
        unit.write_text('[Service]\nType=simple\n'+thunar_unit.read_text())
        # Verify using systemd's real parser: executable paths treat $ differently
        # from command arguments and still expand % specifiers inside quotes.
        run(['systemd-analyze','--user','verify',unit],env=env)
    assert not (data.parent / 'bin/caja').exists(), 'must not shadow PATH commands'
    second = run([cli, '--apply-hosts'], env=env)
    assert not any(line.startswith('写入 ') for line in second.splitlines()), second
    for desktop in applications.glob('*.desktop'):
        run(['desktop-file-validate', desktop])
    run([cli, '--remove-hosts'], env=env)
    assert service.read_text() == original_service
    assert thunar_service.read_text() == original_thunar_service
    assert thunar_unit.read_text() == original_thunar_unit
    assert caja.read_text() == original_desktop
    assert not list((data / 'quicklook').rglob('quicklook-caja'))
    assert config_file.is_file(), 'uninstall must preserve personal settings'
    assert files(system / 'usr') == before
    for directory in directories + [system / 'usr']:
        directory.chmod(0o755)
    removed = run([*dpkg, '--remove', 'quicklook'])
    assert not cli.exists()
    assert config_file.is_file()
    assert service.read_text() == original_service
    assert caja.read_text() == original_desktop
    resolution = run(['apt-get', '-s', 'install', package])
    results = {'private_dpkg_install_remove': True, 'checksums': True,
               'read_only_install_prefix': True, 'idempotent_user_integration': True,
               'native_services_restored': True, 'user_desktop_restored': True,
               'no_path_shadowing': True, 'apt_dependency_resolution': True}
    output = ROOT / '.local/deb-tests'
    output.mkdir(parents=True, exist_ok=True)
    (output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    (output / 'apt.log').write_text(resolution)
    print('PASS ' + json.dumps(results))
