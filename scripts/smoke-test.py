#!/usr/bin/env python3
"""Exercise the real session DBus service with generated local file fixtures."""
import argparse
import base64
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import time
import zipfile
import zlib

ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser()
parser.add_argument("--profile",default="debug",choices=["debug","release"])
parser.add_argument("--no-screenshots",action="store_true",help="run backend checks while the desktop is locked")
options=parser.parse_args()
CLI=ROOT/"target"/options.profile/"quicklook"
OUT=ROOT/".local/smoke"
FIXTURES=OUT/"files"
FIXTURES.mkdir(parents=True,exist_ok=True)

def run(*args,check=True,input=None):
    return subprocess.run([str(CLI),*map(str,args)],capture_output=True,text=input is None,input=input,check=check,timeout=15)

def status():return json.loads(run("--status").stdout)
def action(name):
    subprocess.run(["gdbus","call","--session","--dest","org.quicklook.Preview","--object-path","/org/quicklook/Preview","--method","org.quicklook.Preview.Action",name],check=True,capture_output=True)

def wait_ready(expected=None,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        value=status()
        if value.get("ready") and (expected is None or value.get("file",{}).get("name")==expected):return value
        time.sleep(.025)
    raise AssertionError(f"Preview timeout: {value}")

def png(path,w=640,h=400):
    def chunk(t,b):return struct.pack("!I",len(b))+t+b+struct.pack("!I",zlib.crc32(t+b)&0xffffffff)
    data=b"".join(b"\0"+b"".join(bytes((30+120*x//w,60+100*y//h,170,255))for x in range(w))for y in range(h))
    path.write_bytes(b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",struct.pack("!2I5B",w,h,8,6,0,0,0))+chunk(b"IDAT",zlib.compress(data))+chunk(b"IEND",b""))

def pdf(path):
    objects=[b"<< /Type /Catalog /Pages 2 0 R >>",b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>"]
    for page in (1,2):
        content=f"0.08 0.15 0.3 rg 45 620 505 160 re f 1 1 1 rg BT /F1 28 Tf 70 700 Td (Quick Look - page {page}) Tj ET 0 0 0 rg BT /F1 14 Tf 70 560 Td (Native Poppler PDF rendering and selectable text.) Tj ET".encode()
        objects.append(b"<< /Length "+str(len(content)).encode()+b" >>\nstream\n"+content+b"\nendstream")
        if page==1:objects.append(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>")
    objects.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    data=bytearray(b"%PDF-1.4\n");offsets=[0]
    for i,obj in enumerate(objects,1):offsets.append(len(data));data+=f"{i} 0 obj\n".encode()+obj+b"\nendobj\n"
    start=len(data);data+=f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
    for offset in offsets[1:]:data+=f"{offset:010d} 00000 n \n".encode()
    data+=f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode();path.write_bytes(data)

png(FIXTURES/"01-image.png")
(FIXTURES/"02-中文.txt").write_text("你好，世界！\nRust + GTK4 + 原生 Wayland\n"*100)
(FIXTURES/"03-gb18030.txt").write_bytes(("中文 GB18030 编码𠀀测试。\n"*50).encode("gb18030"))
(FIXTURES/"04-markdown.md").write_text("# Quick Look\n\n原生 **Rust + GTK4** 预览。\n\n```rust\nfn main() { println!(\"Wayland\"); }\n```\n\n| 格式 | 状态 |\n|---|---|\n| 图片 | 正常 |\n")
(FIXTURES/"04b-diagram.md").write_text("# 流程图\n\n```mermaid\ngraph TD\n  A[开始] --> B{判断}\n  B -->|是| C[执行]\n  B -->|否| D[跳过]\n```\n")
(FIXTURES/"04c-table.csv").write_text("日期,产品,数量,单价,备注\n2026-01-01,键盘,9,59,\"加急, 需拆单\"\n2026-01-02,鼠标,13,129,\"\"\n2026-01-03,显示器,2,899,正常\n",encoding="utf-8")
(FIXTURES/"04d-table.tsv").write_text("日期\t产品\t数量\n2026-02-01\t键盘\t9\n2026-02-02\t鼠标\t13\n",encoding="utf-8")
(FIXTURES/"04e-semicolon.csv").write_text("日期;产品;金额\n2026-03-01;\"键盘; 黑色\";59\n2026-03-02;鼠标;129\n",encoding="utf-8")
(FIXTURES/"05-data.json").write_text('{"name":"Quick Look","native":true,"formats":["图片","PDF"],"settings":{"cache_mb":200}}')
(FIXTURES/"06-page.html").write_text('<h1>Native HTML</h1><p>WebKitGTK · <strong>script disabled</strong></p><script>document.body.innerHTML="SCRIPT EXECUTED"</script>')
(FIXTURES/"07-binary.bin").write_bytes(bytes(range(256))*8)
(FIXTURES/"08-image.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400"><rect width="640" height="400" rx="28" fill="#172554"/><circle cx="320" cy="170" r="90" fill="#60a5fa"/><text x="320" y="330" text-anchor="middle" font-size="32" fill="white">Quick Look SVG</text></svg>')
(FIXTURES/"09-config.yaml").write_text("name: Quick Look\nnative: true\nformats:\n  - 图片\n  - PDF\n")
(FIXTURES/"10-document.xml").write_text('<preview native="true"><format>PDF</format><format>图片</format></preview>')
(FIXTURES/"11-document.rtf").write_text(r"{\rtf1\ansi Hello \b Linux\b0\par \u20320?\u22909? Rust preview.}")
pdf(FIXTURES/"12-document.pdf")
shutil.copy2(FIXTURES/"12-document.pdf",FIXTURES/"13-illustration.ai")
(FIXTURES/"14-design.eps").write_text("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 400 260\n0.1 0.2 0.4 setrgbcolor 0 0 400 260 rectfill\n1 1 1 setrgbcolor /Helvetica findfont 30 scalefont setfont 40 120 moveto (Quick Look EPS) show\nshowpage\n%%EOF\n")
with zipfile.ZipFile(FIXTURES/"15-archive.zip","w")as archive:
    archive.writestr("中文/hello.txt","包内文件预览\n")
    archive.writestr("../escape.txt","must not be extracted")
    archive.write(FIXTURES/"01-image.png","image.png")
with tarfile.open(FIXTURES/"16-archive.tar","w")as archive:archive.add(FIXTURES/"02-中文.txt",arcname="hello.txt")
# Minimal hand-built OOXML so Office preview can be checked without LibreOffice.
def docx(path):
    ns='xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
    body=(''
        '<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>季度报告</w:t></w:r></w:p>'
        '<w:p><w:r><w:t>正文段落，包含中文与</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>制表符。</w:t></w:r></w:p>'
        '<w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>要点一：收入增长</w:t></w:r></w:p>'
        '<w:tbl>'
        '<w:tr><w:tc><w:p><w:r><w:t>指标</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>数值</w:t></w:r></w:p></w:tc></w:tr>'
        '<w:tr><w:tc><w:p><w:r><w:t>收入</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1200</w:t></w:r></w:p></w:tc></w:tr>'
        '</w:tbl>')
    with zipfile.ZipFile(path,"w",zipfile.ZIP_DEFLATED)as archive:
        archive.writestr("[Content_Types].xml",'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
        archive.writestr("word/document.xml",f'<?xml version="1.0" encoding="UTF-8"?><w:document {ns}><w:body>{body}</w:body></w:document>')

def xlsx(path):
    rels=('<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="worksheet" Target="worksheets/sheet1.xml"/></Relationships>')
    workbook=('<?xml version="1.0" encoding="UTF-8"?><workbook '
        'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="汇总" sheetId="1" r:id="rId1"/></sheets></workbook>')
    shared=('<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        '<si><t>项目</t></si><si><t>数量</t></si><si><t>苹果</t></si></sst>')
    sheet=('<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
        '<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>'
        '<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>12</v></c><c r="D2"><v>3.5</v></c></row>'
        '</sheetData></worksheet>')
    with zipfile.ZipFile(path,"w",zipfile.ZIP_DEFLATED)as archive:
        archive.writestr("[Content_Types].xml",'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
        archive.writestr("xl/workbook.xml",workbook)
        archive.writestr("xl/_rels/workbook.xml.rels",rels)
        archive.writestr("xl/sharedStrings.xml",shared)
        archive.writestr("xl/worksheets/sheet1.xml",sheet)

def pptx(path):
    rels=('<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId2" Type="slide" Target="slides/slide2.xml"/>'
        '<Relationship Id="rId1" Type="slide" Target="slides/slide1.xml"/></Relationships>')
    presentation=('<?xml version="1.0" encoding="UTF-8"?><p:presentation '
        'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<p:sldIdLst><p:sldId id="256" r:id="rId1"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>')
    def slide(title,lines):
        body=''.join(f'<a:p><a:r><a:t>{line}</a:t></a:r></a:p>' for line in [title]+lines)
        return ('<?xml version="1.0" encoding="UTF-8"?><p:sld '
            'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
            'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
            f'<p:cSld><p:spTree><p:sp><p:txBody>{body}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>')
    with zipfile.ZipFile(path,"w",zipfile.ZIP_DEFLATED)as archive:
        archive.writestr("[Content_Types].xml",'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
        archive.writestr("ppt/presentation.xml",presentation)
        archive.writestr("ppt/_rels/presentation.xml.rels",rels)
        archive.writestr("ppt/slides/slide1.xml",slide("产品路线图",["第一季度：内核重构","第二季度：文件管理器接入"]))
        archive.writestr("ppt/slides/slide2.xml",slide("性能目标",["冷启动 80ms 以内","缓存命中 30ms 以内"]))

docx(FIXTURES/"21-document.docx")
xlsx(FIXTURES/"23-workbook.xlsx")
pptx(FIXTURES/"26-deck.pptx")
with zipfile.ZipFile(FIXTURES/"27-nested.zip","w")as archive:
    archive.write(FIXTURES/"21-document.docx","reports/notes.docx")
    archive.write(FIXTURES/"15-archive.zip","reports/inner.zip")
font=next(iter(Path("/usr/share/fonts/truetype").rglob("*.ttf")),None)
if font:shutil.copy2(font,FIXTURES/"17-font.ttf")
w,h=96,64
(FIXTURES/"18-composite.psd").write_bytes(b"8BPS"+struct.pack(">H",1)+b"\0"*6+struct.pack(">HIIHH",3,h,w,8,3)+b"\0"*12+b"\0\0"+bytes([80])*w*h+bytes([130])*w*h+bytes([220])*w*h)
for ext in ("jpg","bmp","tiff","webp","gif","avif"):
    result=subprocess.run(["ffmpeg","-y","-v","error","-i",str(FIXTURES/"01-image.png"),"-vf","scale=320:200","-frames:v","1",str(FIXTURES/f"19-image.{ext}")],capture_output=True,timeout=30)
    if result.returncode:print(f"Fixture encoder unavailable ({ext}): {result.stderr.decode()[:120]}")
subprocess.run(["ffmpeg","-y","-v","error","-f","lavfi","-i","testsrc2=size=480x270:rate=24","-t","2","-c:v","libx264","-pix_fmt","yuv420p",str(FIXTURES/"20-video.mp4")],check=True,capture_output=True,timeout=30)
run("--quit")
for _ in range(50):
    try:
        if not status().get("running"):break
    except subprocess.CalledProcessError:
        pass
    time.sleep(.02)
log=(OUT/"daemon.log").open("w")
process=subprocess.Popen([str(CLI.parent/"quicklookd")],stdout=log,stderr=log)
for _ in range(100):
    if status().get("running"):break
    if process.poll()is not None:raise AssertionError((OUT/"daemon.log").read_text())
    time.sleep(.025)
results=[]
try:
    for path in sorted(FIXTURES.iterdir()):
        run("--show",path)
        value=wait_ready(path.name)
        if value["error"]:raise AssertionError(f"{path.name}: {value['error']}")
        if path.name=="32-orientation.jpg":
            assert value["image"]["source_size"]==[200,320],value
        if value.get("media") is not None:
            time.sleep(.8)
            value=status()
            assert not value["media"]["error"],value
            assert value["media"]["duration"]>0,value
            if path.name.startswith("22-"):
                assert "showwaves" in value["media"]["visualizer"],value
        if path.name in ("24-animation.gif","24-animation.webp","30-long-animation.webp"):
            time.sleep(.3)
            assert status()["animation_frames"]>0,status()
            action("pause");before=status()["animation_frames"]
            time.sleep(.25)
            assert status()["animation_frames"]==before,status()
            action("pause");time.sleep(.25)
            assert status()["animation_frames"]>before,status()
        if not options.no_screenshots and path.suffix in (".png",".pdf",".md",".json",".mp4"):
            time.sleep(.15)
            run("--screenshot",OUT/(path.stem+".png"))
        results.append({"file":path.name,"ms":round(value["request_to_content_ms"],2),"kind":value["file"]["kind"],"media":value.get("media")})
        print("PASS",path.name,round(value["request_to_content_ms"],2),flush=True)
    if (FIXTURES/"29-large-108mp.png").exists():
        run("--show",FIXTURES/"29-large-108mp.png");wait_ready("29-large-108mp.png")
        subprocess.run(["gdbus","call","--session","--dest","org.quicklook.Preview","--object-path","/org/quicklook/Preview","--method","org.quicklook.Preview.Action","actual"],check=True,capture_output=True)
        until=time.monotonic()+20
        while time.monotonic()<until:
            value=status()
            if value.get("image",{}).get("detail"):break
            time.sleep(.05)
        assert value["image"]["source_size"]==[12000,9000],value
        assert value["image"]["detail"] and max(value["image"]["detail"]["pixels"])<=2048,value
        assert abs(value["image"]["detail"]["rect"][2]-value["image"]["viewport"][0])<=2,value
        (OUT/"large-image-result.json").write_text(json.dumps(value["image"],indent=2)+"\n")
        print("PASS 108 MP image: original-pixel zoom with bounded region refinement",flush=True)
    run("--show",FIXTURES/"12-document.pdf");wait_ready("12-document.pdf")
    action("thumbnails");assert status()["pdf_thumbnails"]
    run("--page",2);value=wait_ready("12-document.pdf");assert value["page"]==2 and value["pages"]==2 and value["pdf_thumbnails"]
    action("continuous");assert status()["pdf_continuous"]
    action("continuous");wait_ready("12-document.pdf");assert not status()["pdf_continuous"]
    run("--show",FIXTURES/"15-archive.zip");wait_ready("15-archive.zip")
    run("--extract",0);value=wait_ready("hello.txt");assert not value["error"]
    run("--show",FIXTURES/"15-archive.zip");wait_ready("15-archive.zip")
    run("--extract",1);value=wait_ready();assert value["error"]
    for name in ("21-document.docx","23-workbook.xlsx","26-deck.pptx"):
        run("--show",FIXTURES/name);value=wait_ready(name)
        assert value["body"]=="office" and not value["error"],value
    run("--show",FIXTURES/"27-nested.zip");wait_ready("27-nested.zip")
    run("--extract",0);value=wait_ready("notes.docx")
    assert value["body"]=="office" and value["nested_depth"]==1,value
    run("--back");value=wait_ready("27-nested.zip");assert value["nested_depth"]==0,value
    print("PASS Office docx/xlsx/pptx bodies, nested archive extraction and back navigation")
    run("--show",FIXTURES/"04b-diagram.md");value=wait_ready("04b-diagram.md")
    assert value["body"]=="markdown" and not value["error"],value
    deadline=time.time()+10
    while time.time()<deadline and not status().get("mermaid"):
        time.sleep(0.15)
    value=status();assert value["mermaid"].startswith("ok:"),value
    run("--show",FIXTURES/"04-markdown.md");value=wait_ready("04-markdown.md")
    assert value["mermaid"]=="",value
    print("PASS Markdown Mermaid flowchart rendering (and plain Markdown stays JS-free)")
    for name in ("04c-table.csv","04d-table.tsv","04e-semicolon.csv"):
        run("--show",FIXTURES/name);value=wait_ready(name)
        assert value["body"]=="csv" and not value["error"],value
    print("PASS CSV/TSV preview renders as a table (comma, tab, semicolon sniffing)")

    run("--show",FIXTURES/"01-image.png",FIXTURES/"02-中文.txt");value=wait_ready();assert value["grid"] and value["selection_count"]==2
    run("--next");value=wait_ready("02-中文.txt");assert not value["grid"]
    run("--pin");run("--prev");assert status()["file"]["name"]=="02-中文.txt"
    run("--unpin");run("--prev");wait_ready("01-image.png")
    run(FIXTURES/"01-image.png",FIXTURES/"02-中文.txt");assert not status()["visible"]
    run("--show",FIXTURES/"01-image.png");wait_ready("01-image.png")
    run("--show",FIXTURES/"01-image.png");assert status()["visible"]
    run("--close");value=status();assert not value["visible"] and value["cache_bytes"]==0
    print("PASS PDF paging, archive extraction/path protection, multi-selection, navigation, pin, toggle, close/cache release")
finally:
    run("--close",check=False)
    (OUT/"results.json").write_text(json.dumps(results,ensure_ascii=False,indent=2)+"\n")
    log.flush()
