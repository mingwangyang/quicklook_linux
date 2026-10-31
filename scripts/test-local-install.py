#!/usr/bin/env python3
"""Verify source install/reinstall/removal in private HOME/XDG directories, without sudo."""
from pathlib import Path
import os,subprocess,tempfile
import niri_launcher
root=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='quicklook-source-install-') as temporary:
    base=Path(temporary);home=base/'home';prefix=base/'prefix';config=home/'config';data=home/'data'
    (config/'quicklook').mkdir(parents=True);(data/'applications').mkdir(parents=True)
    (config/'quicklook/config.toml').write_text('hijack_native = true\n')
    (config/'niri').mkdir()
    (config/'niri/config.kdl').write_text('binds {\n    Mod+E { spawn "true"; }\n}\n')
    (prefix/'bin').mkdir(parents=True)
    helper=prefix/'bin/launch-file-manager'
    original_helper='#!/bin/sh\n# My input-method workaround\n'+niri_launcher.COMMAND+'\n'
    helper.write_text(original_helper);helper.chmod(0o755)
    desktop=data/'applications/caja-browser.desktop'
    original='[Desktop Entry]\nType=Application\nName=My Caja\nExec=caja --no-desktop %U\n'
    desktop.write_text(original)
    env=dict(os.environ,HOME=str(home),QUICKLOOK_PREFIX=str(prefix),XDG_CONFIG_HOME=str(config),XDG_DATA_HOME=str(data),XDG_CACHE_HOME=str(home/'cache'),DBUS_SESSION_BUS_ADDRESS='unix:path='+str(base/'no-bus'))
    env.pop('VIPSHOME',None)
    def run(*args):
        result=subprocess.run(args,env=env,capture_output=True,text=True,timeout=60,cwd=root)
        assert result.returncode==0, result.stdout+result.stderr
        return result.stdout
    options=('python3','scripts/install-local.py','--with-file-managers','--with-niri','--no-start','--no-autostart')
    run(*options)
    assert 'quicklook-caja' in desktop.read_text()
    assert (data/'quicklook/launchers/quicklook-caja').is_file()
    assert 'quicklook-thunar' in helper.read_text()
    assert niri_launcher.remove(helper.read_text())==original_helper
    run(*options)
    assert helper.read_text().count(niri_launcher.BEGIN)==1
    # An unrelated later user edit survives uninstall too.
    helper.write_text(helper.read_text().replace('# My input-method workaround','# My revised input-method workaround'))
    report=run('python3','scripts/uninstall-local.py')
    assert not (prefix/'bin/quicklook').exists(),report
    assert not (data/'quicklook/launchers/quicklook-caja').exists(),report
    assert desktop.read_text()==original,report
    assert (config/'quicklook/config.toml').exists()
    assert helper.read_text()==original_helper.replace('# My input-method workaround','# My revised input-method workaround')
    assert not (config/'systemd/user/thunar.service.d/quicklook.conf').exists()
    assert not (data/'dbus-1/services/org.xfce.Thunar.service').exists()
    print('PASS local install, repeat install, uninstall, Caja and niri launcher restored, Thunar activation removed, preferences preserved')
