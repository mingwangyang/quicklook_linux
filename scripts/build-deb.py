#!/usr/bin/env python3
"""Build a native deb using dpkg's ELF dependency analysis; never edits the host.

Run on the oldest supported distro (currently Ubuntu 24.04 / Mint 22.x).
Libraries remain distribution-managed. Maintainer scripts only touch the
installing user's own Quick Look-managed integration files, never host config
beyond the managed window-rule include.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BINS = ('quicklook', 'quicklookd', 'quicklook-viewer', 'quicklook-settings',
        'quicklook-nautilus-bridge', 'quicklook-clipboard')
LIBS = tuple(f'libquicklook_{host}.so' for host in ('nemo', 'thunar', 'caja'))


def run(command, **kwargs):
    return subprocess.run(list(map(str, command)), check=True, text=True,
                          capture_output=True, **kwargs).stdout.strip()


def build(args):
    for tool in ('dpkg-deb', 'dpkg-shlibdeps', 'dpkg-architecture', 'readelf', 'strip'):
        if not shutil.which(tool):
            raise SystemExit(f'Missing {tool}; install dpkg-dev and binutils.')
    env = dict(os.environ)
    for key in ('LD_PRELOAD', 'LD_LIBRARY_PATH', 'VIPSHOME'):
        env.pop(key, None)
    if not args.no_build:
        print('Building Rust release workspace…', flush=True)
        subprocess.run(['cargo', 'build', '--locked', '--release', '--workspace'],
                       cwd=ROOT, env=env, check=True)
    cargo_version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    display_version = 'V' + cargo_version
    version = (args.version or cargo_version).removeprefix('V').removeprefix('v')
    if not re.fullmatch(r'[0-9][A-Za-z0-9.+:~\-]*', version):
        raise SystemExit('Invalid Debian version')
    arch = run(['dpkg', '--print-architecture'])
    epoch = env.get('SOURCE_DATE_EPOCH') or run(['git', 'log', '-1', '--format=%ct'], cwd=ROOT)
    env['SOURCE_DATE_EPOCH'] = epoch
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    result = output / f'quicklook_V{version}_{arch}.deb'
    with tempfile.TemporaryDirectory(prefix='quicklook-deb-') as temporary:
        work = Path(temporary)
        stage = work / 'package'
        metadata = stage / 'DEBIAN'
        metadata.mkdir(parents=True)

        def copy(source, relative, executable=False):
            target = stage / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
            target.chmod(0o755 if executable else 0o644)
            return target

        elf = []
        for name in (*BINS, *LIBS):
            source = ROOT / 'target/release' / name
            if not source.is_file():
                raise SystemExit(f'Missing {source}; build --release --workspace first.')
            dynamic = run(['readelf', '-d', source], env=env)
            if '(RPATH)' in dynamic or '(RUNPATH)' in dynamic:
                raise SystemExit(f'Refusing nonportable runtime library path in {source}')
            relative = ('usr/bin/' if name in BINS else 'usr/lib/quicklook/') + name
            target = copy(source, relative, name in BINS)
            run(['strip', '--strip-unneeded', target])
            elf.append(target)
        actual_version = run([stage / 'usr/bin/quicklook', '--version'], env=env)
        if actual_version != 'quicklook ' + display_version:
            raise SystemExit(f'Stale release artifacts: expected {display_version}, got {actual_version}. Rebuild first.')
        for name in ('org.quicklook.Preview.desktop', 'org.quicklook.Preview.Settings.desktop'):
            target = copy(ROOT / 'packaging/debian' / name, 'usr/share/applications/' + name)
            if shutil.which('desktop-file-validate'):
                run(['desktop-file-validate', target])
        copy(ROOT / 'packaging/debian/org.quicklook.Preview.service',
             'usr/share/dbus-1/services/org.quicklook.Preview.service')
        copy(ROOT / 'integration/icons/org.quicklook.Preview.svg',
             'usr/share/icons/hicolor/scalable/apps/org.quicklook.Preview.svg')
        for icon in sorted((ROOT / 'integration/icons/hicolor').rglob('*.png')):
            copy(icon, 'usr/share/icons/hicolor/' + str(icon.relative_to(ROOT / 'integration/icons/hicolor')))
        copy(ROOT / 'packaging/debian/copyright', 'usr/share/doc/quicklook/copyright')
        for name in ('README.md', 'docs/PACKAGING.md', 'docs/FILE_MANAGERS.md', 'docs/REVIEW.md'):
            target = stage / 'usr/share/doc/quicklook' / (Path(name).name + '.gz')
            target.write_bytes(gzip.compress((ROOT / name).read_bytes(), mtime=0))
            target.chmod(0o644)
        # Rust crates are linked into the executable. Ship their copyright/license
        # notices as well as the application's GPL notice; no cached paths are exposed.
        metadata_json = json.loads(run(['cargo', 'metadata', '--locked', '--format-version=1'], cwd=ROOT, env=env))
        notices = []
        members = set(metadata_json['workspace_members'])
        for dependency in sorted(metadata_json['packages'], key=lambda item: (item['name'], item['version'])):
            if dependency['id'] in members:
                continue
            notices.append(dependency['name'] + ' ' + dependency['version'] + '\n'
                           + 'License: ' + (dependency.get('license') or 'see license below') + '\n'
                           + 'Source: ' + (dependency.get('repository') or dependency.get('source') or '') + '\n')
            directory = Path(dependency['manifest_path']).parent
            candidates = {path for pattern in ('LICENSE*', 'LICENCE*', 'COPYING*', 'NOTICE*', 'license*')
                          for path in directory.glob(pattern) if path.is_file()}
            if dependency.get('license_file'):
                candidates.add(directory / dependency['license_file'])
            for license_path in sorted(candidates):
                if license_path.is_file():
                    notices.append(license_path.name + '\n' + license_path.read_text(errors='replace') + '\n')
        notice_file = stage / 'usr/share/doc/quicklook/third-party-notices.txt.gz'
        notice_file.write_bytes(gzip.compress('\n'.join(notices).encode(), mtime=0))
        notice_file.chmod(0o644)
        # dpkg-shlibdeps requires a source control context. It computes ABI minimums
        # from distro symbols rather than copying this machine's installed versions.
        source_control = work / 'debian/control'
        source_control.parent.mkdir()
        source_control.write_text('Source: quicklook\nSection: utils\nPriority: optional\n'
                                  'Maintainer: Quick Look Linux Packagers <quicklook@example.org>\n\nPackage: quicklook\n'
                                  'Architecture: any\nDescription: Native file previewer\n')
        deps = run(['dpkg-shlibdeps', '-O', *('-e' + str(path) for path in elf)], cwd=work, env=env)
        depends = next(line.removeprefix('shlibs:Depends=') for line in deps.splitlines()
                       if line.startswith('shlibs:Depends='))
        # The optional adapters dlopen GTK3. gdbus is used by generated launchers;
        # Ghostscript implements EPS, and wl-copy is the clipboard fallback.
        depends += ', libgtk-3-0t64 (>= 3.24), libglib2.0-bin, bash, coreutils, procps, sed, grep, ghostscript, wl-clipboard, util-linux'
        installed_size = sum(path.stat().st_size for path in stage.rglob('*') if path.is_file())
        (metadata / 'control').write_text(
            f'Package: quicklook\nVersion: {version}\nArchitecture: {arch}\n'
            'Section: utils\nPriority: optional\nMaintainer: Quick Look Linux Packagers <quicklook@example.org>\n'
            f'Installed-Size: {(installed_size + 1023) // 1024}\nDepends: {depends}\n'
            'Recommends: zenity\nSuggests: python3-caja, libheif-plugin-libde265, libheif-plugin-aomdec\n'
            'Description: Native Wayland-friendly file previewer\n'
            ' Rust and GTK4 previews for images, media, PDF, text, office files and archives.\n'
            ' File manager integration and keyboard takeover are optional user settings.\n')
        # 安装/卸载时以发起安装的用户身份补/撤用户级接入。脚本只调用 quicklook
        # 自己维护的受管文件，幂等且可撤销。
        scripts = []
        for name in ('postinst', 'prerm'):
            target = copy(ROOT / 'packaging/debian' / name, 'DEBIAN/' + name, executable=True)
            scripts.append(target)
        (stage / 'usr/share/doc/quicklook/build-info.json').write_text(json.dumps({
            'version': version, 'display_version': display_version, 'architecture': arch,
            'source_revision': run(['git', 'rev-parse', 'HEAD'], cwd=ROOT),
            'working_tree_dirty': bool(run(['git', 'status', '--porcelain'], cwd=ROOT)),
            'source_date_epoch': int(epoch),
        }, indent=2) + '\n')
        # Checksums are relative to the filesystem root, excluding package metadata.
        (metadata / 'md5sums').write_text(''.join(
            f'{hashlib.md5(path.read_bytes()).hexdigest()}  {path.relative_to(stage)}\n'
            for path in sorted((stage / 'usr').rglob('*')) if path.is_file()))
        # System caches are maintained by Debian's existing shared-mime/desktop/icon
        # triggers. No postinst touches users, starts GUI processes, or injects hosts.
        for path in metadata.iterdir():
            if path in scripts:
                path.chmod(0o755)
            else:
                path.chmod(0o644)
        for directory in stage.rglob('*'):
            if directory.is_dir():
                directory.chmod(0o755)
        run(['dpkg-deb', '--root-owner-group', '--build', stage, result], env=env)
    checksum = hashlib.sha256(result.read_bytes()).hexdigest()
    (output / (result.name + '.sha256')).write_text(f'{checksum}  {result.name}\n')
    print(f'Built: {result}\nSHA256: {checksum}')
    print('Test install resolution: apt-get -s install ' + str(result))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--no-build', action='store_true', help='package existing release artifacts')
    parser.add_argument('--version', help='Debian version; defaults to Cargo version (optional V prefix accepted)')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    build(parser.parse_args())
