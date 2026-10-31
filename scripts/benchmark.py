#!/usr/bin/env python3
"""Measure request/data-ready latency and the application's idle process tree.
This does not measure presentation time on the compositor."""
import argparse, json, statistics, subprocess, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser()
parser.add_argument("--profile",default="release",choices=["debug","release"])
options=parser.parse_args()
BIN=ROOT/"target"/options.profile
FILES=ROOT/".local/smoke/files"
OUT=ROOT/".local"
def cli(*args):
    return subprocess.run([str(BIN/"quicklook"),*map(str,args)],capture_output=True,text=True,check=True,timeout=15).stdout
def status():return json.loads(cli("--status"))
def ready(name,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        value=status()
        if value.get("ready") and value.get("file",{}).get("name")==name:
            assert not value["error"],value
            return value
        time.sleep(.002)
    raise AssertionError(value)
def show(path):
    start=time.perf_counter()
    cli("--show",path)
    value=ready(path.name)
    return {"request_roundtrip_to_ready_ms":round((time.perf_counter()-start)*1000,2),"viewer_data_ready_ms":round(value["request_to_content_ms"],2)}
def summarize(rows):
    return {key:{"median":round(statistics.median(r[key]for r in rows),2),"max":max(r[key]for r in rows),"samples":[r[key]for r in rows]}for key in rows[0]}
def tree(pid):
    processes={}
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():continue
        try:
            fields=dict(line.split(":",1)for line in (path/"status").read_text().splitlines()if ":"in line)
            processes[int(path.name)]={"pid":int(path.name),"ppid":int(fields["PPid"]),"name":fields["Name"].strip(),"rss_kib":int(fields.get("VmRSS","0 kB").split()[0])}
        except (OSError,ValueError,KeyError):pass
    chosen={pid}
    while True:
        expanded=chosen|{p for p,s in processes.items()if s["ppid"]in chosen}
        if expanded==chosen:break
        chosen=expanded
    return [processes[p]for p in sorted(chosen)if p in processes]
cli("--quit")
time.sleep(.15)
log=(OUT/"benchmark-daemon.log").open("w")
broker=subprocess.Popen([str(BIN/"quicklookd")],stdout=log,stderr=log)
for _ in range(100):
    if status().get("running"):break
    time.sleep(.01)
result={"profile":options.profile,"clock":"monotonic; includes CLI/status polling; no compositor first-frame claim","cold_viewer":show(FILES/"01-image.png")}
warm=[]
for _ in range(12):
    cli("--close")
    warm.append(show(FILES/"01-image.png"))
result["warm_reopen"]=summarize(warm)
# Explicit selection makes ordering deterministic and avoids measuring the directory scan.
cli("--show",FILES/"01-image.png",FILES/"02-中文.txt")
time.sleep(.25)
switches=[]
for index in range(20):
    start=time.perf_counter()
    cli("--next" if index%2==0 else "--prev")
    value=ready("02-中文.txt" if index%2==0 else "01-image.png")
    switches.append({"request_roundtrip_to_ready_ms":round((time.perf_counter()-start)*1000,2),"viewer_data_ready_ms":round(value["request_to_content_ms"],2)})
result["cached_navigation"]=summarize(switches)
text=FILES/"31-ten-thousand-lines.rs"
text.write_text("".join(f"// line {i}: 中文编码验证\nlet value_{i} = {i};\n"for i in range(10000)))
result["text_10000_lines"]=show(text)
cli("--close")
until=time.monotonic()+10
while time.monotonic()<until:
    value=status()
    if not value.get("viewer_running"):break
    time.sleep(.1)
result["after_close_processes"]=tree(broker.pid)
result["idle_tree_rss_kib"]=sum(p["rss_kib"]for p in result["after_close_processes"])
result["idle_below_60_mib"]=result["idle_tree_rss_kib"]<60*1024
assert result["idle_below_60_mib"],result
path=OUT/"benchmark.json"
path.write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
print(json.dumps(result,ensure_ascii=False,indent=2))
