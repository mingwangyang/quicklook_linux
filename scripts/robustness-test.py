#!/usr/bin/env python3
"""Malformed files, cancellation races, path handling and archive links."""
import json,os,subprocess,tempfile,time,zipfile,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
CLI=ROOT/"target/release/quicklook"
def cli(*args,input=None,check=True):
    return subprocess.run([str(CLI),*map(str,args)],input=input,capture_output=True,check=check,timeout=10)
def status():return json.loads(cli("--status").stdout)
def ready(name,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        value=status()
        if value.get("ready") and value.get("file",{}).get("name")==name:return value
        time.sleep(.02)
    raise AssertionError(value)
cli("--quit")
for _ in range(100):
    try:
        if not status().get("running"):break
    except subprocess.CalledProcessError:pass
    time.sleep(.02)
time.sleep(.1)
log=(ROOT/".local/robustness-daemon.log").open("w")
broker=subprocess.Popen([str(CLI.parent/"quicklookd")],stdout=log,stderr=log)
for _ in range(100):
    if status().get("running"):break
    time.sleep(.02)
assert Path(f"/proc/{status()['service_pid']}/exe").resolve()==CLI.parent/"quicklookd"
result={}
with tempfile.TemporaryDirectory(prefix="robustness-",dir=ROOT/".local")as directory:
    directory=Path(directory)
    for suffix,header in [("png",b"\x89PNG\r\n\x1a\n"),("pdf",b"%PDF-1.7\n"),("psd",b"8BPS"),("webp",b"RIFF\0\0\0\0WEBP")]:
        path=directory/f"broken.{suffix}";path.write_bytes(header+b"broken")
        cli("--show",path)
        value=ready(path.name)
        assert value["error"],value
    result["corrupt_files_recover"]=True
    pipe=directory/"named-pipe";os.mkfifo(pipe)
    assert cli("--show",pipe,check=False).returncode!=0
    assert cli("--show","https://example.invalid/file",check=False).returncode!=0
    result["devices_and_remote_uris_rejected"]=True
    files=[]
    for name in ["中文 a\n#?.txt","name $(touch NEVER_EXECUTED).txt"]:
        path=directory/name;path.write_text("literal file name\n");files.append(path)
    cli("--stdin0",input=b"\0".join(os.fsencode(p)for p in files)+b"\0")
    assert status()["grid"] and status()["selection_count"]==2
    assert not (ROOT/"NEVER_EXECUTED").exists()
    invalid=directory/os.fsdecode(b"non-utf8-\xff.txt");invalid.write_text("hello")
    cli("--show",invalid)
    until=time.monotonic()+10
    while time.monotonic()<until:
        value=status()
        if value.get("ready")and "%FF"in value.get("file",{}).get("uri",""):break
        time.sleep(.02)
    assert not value["error"] and "%FF"in value["file"]["uri"],value
    result["literal_filenames_and_nul_selection"]=True
    result["non_utf8_filename_status"]=True
    archive=directory/"links.zip"
    with zipfile.ZipFile(archive,"w")as output:
        link=zipfile.ZipInfo("symlink");link.create_system=3;link.external_attr=0o120777<<16
        output.writestr(link,"/etc/passwd")
    cli("--show",archive);ready(archive.name)
    cli("--extract",0);assert ready(archive.name)["error"]
    result["archive_symlinks_rejected"]=True
    fixture=ROOT/".local/smoke/files"
    original=(fixture/"19-image.webp").read_bytes()
    large_static=directory/"large-static.webp"
    padding=33*1024*1024
    with large_static.open("wb")as output:
        output.write(original[:4]+struct.pack("<I",len(original)+8+padding-8)+original[8:]+b"JUNK"+struct.pack("<I",padding))
        for _ in range(33):output.write(b"\0"*(1024*1024))
    cli("--show",large_static);value=ready(large_static.name)
    assert not value["error"] and value["image"]["source_size"]==[320,200],value
    result["large_static_webp_retains_original_pixel_mode"]=True
    for index in range(30):
        cli("--show",fixture/("29-large-108mp.png"if index%3==0 else "12-document.pdf"if index%3==1 else "02-中文.txt"))
    final=fixture/"01-image.png"
    cli("--show",final)
    value=ready(final.name)
    assert not value["error"],value
    result["rapid_navigation_cancels_stale_results"]=True
    gallery=directory/"gallery";gallery.mkdir()
    for i in range(1000):os.link(final,gallery/f"{i:04}.png")
    cli("--show",gallery/"0000.png");ready("0000.png")
    until=time.monotonic()+10
    while status()["sequence_count"]<1000 and time.monotonic()<until:time.sleep(.02)
    assert status()["sequence_count"]==1000,status()
    start=time.monotonic()
    for _ in range(80):cli("--next")
    assert not ready("0080.png")["error"]
    result["thousand_image_directory"]={"entries":1000,"switches":80,"elapsed_ms":round((time.monotonic()-start)*1000,2)}
cli("--close")
(ROOT/".local/robustness-result.json").write_text(json.dumps(result,indent=2)+"\n")
print("PASS",json.dumps(result))
