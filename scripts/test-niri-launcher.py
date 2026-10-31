#!/usr/bin/env python3
"""Run the migrated helper with fake hosts, preserving environment and arguments."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import niri_launcher

with tempfile.TemporaryDirectory(prefix='quicklook-niri-launcher-') as temporary:
    base=Path(temporary)
    # These characters must stay literal even in an executable shell fragment.
    loader=base/'data $HOME %i with spaces'/'quicklook-thunar'
    loader.parent.mkdir()
    fake='#!/usr/bin/python3\nimport json,os,sys\nprint(json.dumps([__file__,sys.argv[1:],os.environ.get("GDK_BACKEND"),os.environ.get("GTK_IM_MODULE")]))\n'
    loader.write_text(fake);loader.chmod(0o755)
    thunar=base/'thunar';thunar.write_text(fake);thunar.chmod(0o755)
    original='#!/bin/sh\n# input-method workaround\n'+niri_launcher.COMMAND+'\n'
    migrated=niri_launcher.integrate(original,loader)
    assert niri_launcher.integrate(migrated,loader)==migrated
    assert niri_launcher.remove(migrated)==original
    assert niri_launcher.integrate('#!/bin/sh\nexec dolphin "$@"\n',loader) is None
    helper=base/'helper';helper.write_text(migrated);helper.chmod(0o755)
    env=dict(os.environ,PATH=str(base)+':/usr/bin:/bin',GDK_BACKEND='x11',GTK_IM_MODULE='fcitx')
    arguments=['/tmp/name with spaces.png','/tmp/$(touch sentinel).txt']
    def invoke():
        return json.loads(subprocess.check_output([helper,*arguments],env=env,text=True))
    assert invoke()==[str(loader),arguments,'wayland',None]
    loader.unlink()  # disabling/removing host integration falls back cleanly
    assert invoke()==[str(thunar),arguments,'wayland',None]
    for invalid in (niri_launcher.BEGIN+original,niri_launcher.END+original,niri_launcher.END+niri_launcher.BEGIN):
        try: niri_launcher.integrate(invalid,loader)
        except ValueError: pass
        else: raise AssertionError('accepted broken managed block')
    print('PASS known helper migration, repeat apply, removal, fallback, literal paths and input-method environment')
