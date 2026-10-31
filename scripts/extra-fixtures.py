#!/usr/bin/env python3
"""Generate extra codecs and a RAW without an embedded JPEG; optional upstream samples stay local."""
import argparse, ctypes, struct, subprocess, urllib.request, binascii, zlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser()
parser.add_argument('--download-samples',action='store_true')
args=parser.parse_args()
OUT=ROOT/'.local/smoke/files'
OUT.mkdir(parents=True,exist_ok=True)
def ffmpeg(*arguments):
    subprocess.run(['ffmpeg','-y','-v','error',*map(str,arguments)],capture_output=True,check=True,timeout=60)
ffmpeg('-i',OUT/'01-image.png','-vf','scale=320:200','-frames:v',1,OUT/'21-image.jxl')
for suffix in ['wav','mp3','flac','ogg','m4a','opus']:
    ffmpeg('-f','lavfi','-i','anullsrc=r=44100:cl=stereo','-t',2,OUT/f'22-silence.{suffix}')
for suffix in ['mkv','mov']:
    ffmpeg('-i',OUT/'20-video.mp4','-c','copy',OUT/f'23-video.{suffix}')
ffmpeg('-f','lavfi','-i','testsrc2=size=160x100:rate=10','-t',1,'-c:v','libvpx-vp9',OUT/'23-video.webm')
ffmpeg('-f','lavfi','-i','testsrc2=size=160x100:rate=5','-t',1,'-loop',0,OUT/'24-animation.gif')
ffmpeg('-f','lavfi','-i','testsrc2=size=160x100:rate=5','-t',1,'-loop',0,OUT/'24-animation.webp')

# Minimal TIFF/DNG Bayer fixture: exercises full LibRaw decoding, not JPEG extraction.
w,h=160,100
tags={
256:(4,1,struct.pack('<I',w)),257:(4,1,struct.pack('<I',h)),
258:(3,1,struct.pack('<H',16)),259:(3,1,struct.pack('<H',1)),262:(3,1,struct.pack('<H',32803)),
271:(2,10,b'QuickLook\0'),272:(2,10,b'Synthetic\0'),273:(4,1,b'\0'*4),
277:(3,1,struct.pack('<H',1)),278:(4,1,struct.pack('<I',h)),279:(4,1,struct.pack('<I',w*h*2)),
284:(3,1,struct.pack('<H',1)),33421:(3,2,struct.pack('<HH',2,2)),33422:(1,4,bytes([0,1,1,2])),
50706:(1,4,bytes([1,4,0,0])),50707:(1,4,bytes([1,3,0,0])),50708:(2,10,b'Synthetic\0'),
50710:(1,3,bytes([0,1,2])),50711:(3,1,struct.pack('<H',1)),50713:(3,2,struct.pack('<HH',1,1)),
50714:(5,1,struct.pack('<II',0,1)),50717:(4,1,struct.pack('<I',65535)),
50719:(4,2,struct.pack('<II',0,0)),50720:(4,2,struct.pack('<II',w,h)),
50721:(10,9,b''.join(struct.pack('<ii',int(i in [0,4,8]),1)for i in range(9))),
50728:(5,3,struct.pack('<IIIIII',1,1,1,1,1,1)),50778:(3,1,struct.pack('<H',21)),
50829:(4,4,struct.pack('<IIII',0,0,h,w))}
start=8+2+len(tags)*12+4
extras=bytearray()
entries=[]
for tag,(kind,count,data)in sorted(tags.items()):
    if len(data)>4:
        pointer=struct.pack('<I',start+len(extras))
        extras+=data
        if len(extras)%2:extras+=b'\0'
    else:pointer=data.ljust(4,b'\0')
    entries.append((tag,kind,count,pointer))
raw_offset=start+len(extras)
table=b''.join(struct.pack('<HHI',tag,kind,count)+(struct.pack('<I',raw_offset)if tag==273 else pointer)for tag,kind,count,pointer in entries)
pixels=b''.join(struct.pack('<H',15000+20000*x//w+10000*y//h)for y in range(h)for x in range(w))
(OUT/'25-bayer.dng').write_bytes(b'II*\0'+struct.pack('<I',8)+struct.pack('<H',len(tags))+table+b'\0'*4+extras+pixels)

# Write a tiny 7z through the installed libarchive, without installing another utility.
archive=ctypes.CDLL('libarchive.so.13')
def c(name,restype,argtypes):
    function=getattr(archive,name)
    function.restype=restype
    function.argtypes=argtypes
    return function
pointer=ctypes.c_void_p
integer=ctypes.c_int
string=ctypes.c_char_p
new=c('archive_write_new',pointer,[])
format7z=c('archive_write_set_format_7zip',integer,[pointer])
openfile=c('archive_write_open_filename',integer,[pointer,string])
header=c('archive_write_header',integer,[pointer,pointer])
write=c('archive_write_data',ctypes.c_ssize_t,[pointer,pointer,ctypes.c_size_t])
close=c('archive_write_close',integer,[pointer])
free=c('archive_write_free',integer,[pointer])
entry_new=c('archive_entry_new',pointer,[])
entry_free=c('archive_entry_free',None,[pointer])
path=c('archive_entry_set_pathname',None,[pointer,string])
size=c('archive_entry_set_size',None,[pointer,ctypes.c_int64])
kind=c('archive_entry_set_filetype',None,[pointer,ctypes.c_uint])
permissions=c('archive_entry_set_perm',None,[pointer,integer])
a=new()
assert format7z(a)==0
assert openfile(a,str(OUT/'26-archive.7z').encode())==0
e=entry_new()
data=b'Quick Look native 7z preview\n'
path(e,b'hello.txt')
size(e,len(data))
kind(e,0o100000)
permissions(e,0o600)
assert header(a,e)==0
assert write(a,data,len(data))==len(data)
entry_free(e)
close(a)
free(a)

if args.download_samples:
    url='https://raw.githubusercontent.com/strukturag/libheif/v1.17.6/examples/example.heic'
    with urllib.request.urlopen(url,timeout=30)as response:
        (OUT/'27-example.heic').write_bytes(response.read(8*1024*1024))
    url='https://raw.githubusercontent.com/libarchive/libarchive/v3.7.2/libarchive/test/test_read_format_rar.rar.uu'
    with urllib.request.urlopen(url,timeout=30)as response:lines=response.read().splitlines()
    output=bytearray()
    inside=False
    for line in lines:
        if line.startswith(b'begin '):
            inside=True
            continue
        if line==b'end':break
        if inside and line:output+=binascii.a2b_uu(line)
    (OUT/'28-upstream.rar').write_bytes(output)
# 108 MP image, generated by rows so the fixture generator itself stays small.
large=OUT/'29-large-108mp.png'
if not large.exists():
    def png_chunk(kind,data):return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data)&0xffffffff)
    compressor=zlib.compressobj(3)
    row=b'\0'+b''.join(bytes((x%256,(x//16)%256,100))for x in range(12000))
    with large.open('wb')as output:
        output.write(b'\x89PNG\r\n\x1a\n'+png_chunk(b'IHDR',struct.pack('!2I5B',12000,9000,8,2,0,0,0)))
        for _ in range(9000):
            data=compressor.compress(row)
            if data:output.write(png_chunk(b'IDAT',data))
        output.write(png_chunk(b'IDAT',compressor.flush())+png_chunk(b'IEND',b''))
# Longer than the in-memory animation budget: exercises libmpv streaming.
ffmpeg('-f','lavfi','-i','testsrc2=size=80x50:rate=30','-t',9,'-loop',0,OUT/'30-long-animation.gif')
ffmpeg('-f','lavfi','-i','testsrc2=size=80x50:rate=30','-t',9,'-loop',0,OUT/'30-long-animation.webp')
ffmpeg('-i',OUT/'01-image.png','-vf','scale=32:32','-frames:v',1,OUT/'19-image.ico')
# Rotated EXIF JPEG: original sensor pixels 320x200, displayed size 200x320.
original=(OUT/'19-image.jpg').read_bytes()
tiff=b'II*\0'+struct.pack('<I',8)+struct.pack('<H',1)+struct.pack('<HHI',274,3,1)+struct.pack('<H',6)+b'\0\0'+b'\0'*4
segment=b'Exif\0\0'+tiff
(OUT/'32-orientation.jpg').write_bytes(original[:2]+b'\xff\xe1'+struct.pack('!H',len(segment)+2)+segment+original[2:])
# Optional upstream files stay out of Git. Pin their source revisions.
if args.download_samples:
    samples=[
      ('33-canon.CR2','https://raw.githubusercontent.com/letmaik/rawpy/401a9e5e4cd2348b8d11bdddd567915de07c0a83/test/M0054341_01_00005.cr2'),
      ('34-nikon.NEF','https://raw.githubusercontent.com/letmaik/rawpy/401a9e5e4cd2348b8d11bdddd567915de07c0a83/test/iss030e122639.NEF'),
      ('35-sony.ARW','https://raw.githubusercontent.com/f-spot/raw-samples/54425e2cd53ec17d5fd88ee3277d03f54bf1fe84/ARW/RAW_SONY_A700.ARW')]
    for name,url in samples:
        if not (OUT/name).exists():
            with urllib.request.urlopen(url,timeout=60)as response:data=response.read(64*1024*1024+1)
            assert len(data)<=64*1024*1024
            (OUT/name).write_bytes(data)
print('Extended local fixtures ready:',OUT)
