#!/usr/bin/env python3
"""Validate the Rust adapter in an isolated Nemo instance; never restart the user's Nemo."""
import argparse,json,os,signal,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--profile',default='debug',choices=['debug','release']);args=parser.parse_args()
BIN=ROOT/'target'/args.profile
OUT=ROOT/'.local';OUT.mkdir(exist_ok=True)
FILES=OUT/'smoke/files'
assert FILES.is_dir(),'Run scripts/smoke-test.py first'
def cli(*args):return subprocess.run([str(BIN/'quicklook'),*map(str,args)],check=True,text=True,capture_output=True,timeout=15).stdout
def status():return json.loads(cli('--status'))
def wait(condition,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        try:
            value=condition()
            if value:return value
        except (subprocess.CalledProcessError,json.JSONDecodeError):pass
        time.sleep(.05)
    raise AssertionError('Nemo adapter test timed out; see .local/nemo-adapter-test.log')
def expect(condition,what,timeout=20):
    """失败时把守护进程看到的状态和适配器自己的诊断一起打出来，不然只能靠猜。"""
    try:
        wait(condition,timeout=timeout)
    except AssertionError:
        print(f'失败：{what}；状态 {json.dumps(status(),ensure_ascii=False)}',flush=True)
        raise
cli('--quit');time.sleep(.3)
broker_log=(OUT/'nemo-broker-test.log').open('w')
broker=subprocess.Popen([str(BIN/'quicklookd')],stdout=broker_log,stderr=broker_log)
wait(lambda:status().get('running'))
value=0xcbf29ce484222325
for byte in Path('/usr/bin/nemo').read_bytes():value=((value^byte)*0x100000001b3)&0xffffffffffffffff
assert f'{value:016x}'==json.loads((ROOT/'integration/nemo-host.json').read_text())['fingerprint_fnv1a64'],'Nemo build has not been verified'
address=OUT/'nemo-private-address';address.unlink(missing_ok=True)
environment=dict(os.environ,QUICKLOOK_DBUS_ADDRESS=os.environ['DBUS_SESSION_BUS_ADDRESS'],QUICKLOOK_NEMO_FINGERPRINT=f'{value:016x}',QUICKLOOK_ADAPTER_TEST='1')
environment.update(NO_AT_BRIDGE='1',GTK_A11Y='none',GIO_USE_VFS='local',GIO_USE_VOLUME_MONITOR='unix')
# The test selection helper intentionally targets GtkTreeView. Force list view in
# a private keyfile backend so the test is independent of the user's Nemo layout.
private_config=OUT/'nemo-test-config'
private_config.mkdir(exist_ok=True)
environment.update(GSETTINGS_BACKEND='keyfile',XDG_CONFIG_HOME=str(private_config))
subprocess.run(['gsettings','set','org.nemo.preferences','default-folder-viewer','list-view'],env=environment,check=True)
program="import os; from pathlib import Path; Path(os.environ['QUICKLOOK_TEST_ADDRESS']).write_text(os.environ['DBUS_SESSION_BUS_ADDRESS']); os.environ['LD_PRELOAD']=os.environ['QUICKLOOK_TEST_MODULE']; os.execv('/usr/bin/nemo',['/usr/bin/nemo','--no-desktop',os.environ['QUICKLOOK_TEST_FILES']])"
environment.update(QUICKLOOK_TEST_ADDRESS=str(address),QUICKLOOK_TEST_MODULE=str(BIN/'libquicklook_nemo.so'),QUICKLOOK_TEST_FILES=str(FILES))
log=(OUT/'nemo-adapter-test.log').open('w')
process=subprocess.Popen(['dbus-run-session','--','python3','-c',program],env=environment,stdout=log,stderr=log,start_new_session=True)
try:
    wait(address.exists)
    private=address.read_text();env=dict(os.environ,DBUS_SESSION_BUS_ADDRESS=private)
    def call(method,*args,dest='org.quicklook.NemoAdapter',path='/org/quicklook/NemoAdapter'):
        return subprocess.run(['gdbus','call','--session','--dest',dest,'--object-path',path,'--method',f'{dest}.{method}',*map(str,args)],env=env,capture_output=True,text=True,check=True,timeout=5).stdout
    def select(*names):
        uris=[(FILES/name).resolve().as_uri()for name in names]
        def attempt():
            call('Select',json.dumps(uris))
            return all(uri in call('Selection')for uri in uris)
        try:wait(attempt,timeout=8)
        except AssertionError:
            print('Nemo diagnostics:',call('Debug'),flush=True)
            raise
    wait(lambda:'org.quicklook.NemoAdapter' in subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.ListNames'],env=env,capture_output=True,text=True).stdout)
    select('01-image.png')
    assert 'true' in call('Key',32)
    expect(lambda:status().get('ready') and status().get('file',{}).get('name')=='01-image.png','单个选中项没出预览')
    assert 'true' in call('Key',65307)
    expect(lambda:not status().get('visible'),'Esc 没关掉预览')
    select('01-image.png','02-中文.txt')
    assert 'true' in call('Key',32)
    expect(lambda:status().get('grid') and status().get('selection_count')==2,'两个选中项没有出网格')
    select('05-data.json');call('Sync')
    wait(lambda:status().get('ready') and status().get('file',{}).get('name')=='05-data.json')
    cli('--pin');time.sleep(.3)
    select('02-中文.txt');call('Sync');time.sleep(.3)
    assert status()['file']['name']=='05-data.json'
    cli('--unpin');time.sleep(.3);call('Sync')
    wait(lambda:status().get('ready') and status().get('file',{}).get('name')=='02-中文.txt')
    call('Key',65307);wait(lambda:not status().get('visible'))
    result={'nemo':'6.6.3','fingerprint':f'{value:016x}','single_space':True,'escape':True,'space_multi_selection':True,'selection_sync':True,'pin':True,'isolated_session':True}
    (OUT/'nemo-adapter-result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print('PASS native Rust Nemo adapter: single/multiple selection, Space, Esc, synchronization and pin')
finally:
    try:os.killpg(process.pid,signal.SIGTERM)
    except ProcessLookupError:pass
    process.wait(timeout=10);log.close()
