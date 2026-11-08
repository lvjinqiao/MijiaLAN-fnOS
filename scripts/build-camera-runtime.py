#!/usr/bin/env python3
"""Build the minimal Linux camera runtime from the published source bundle.
Requires Python 3, make, Zig 0.17.0 and the archives named below.
Usage: python3 scripts/build-camera-runtime.py --zig /path/to/zig --work /tmp/camera --sources /path/to/video-sources
"""
import argparse, os, shutil, subprocess, tarfile
from pathlib import Path
p=argparse.ArgumentParser()
p.add_argument('--zig', required=True)
p.add_argument('--work', required=True)
p.add_argument('--sources', required=True)
a=p.parse_args()
w=Path(a.work).resolve(); src=Path(a.sources).resolve(); zig=Path(a.zig).resolve()
w.mkdir(parents=True,exist_ok=True)
(w/'src').mkdir(exist_ok=True); (w/'bin').mkdir(exist_ok=True)
env=dict(os.environ, ZIG_GLOBAL_CACHE_DIR=str(w/'zig-cache'), ZIG_LOCAL_CACHE_DIR=str(w/'zig-local'))
for archive in ['ffmpeg-n7.1.1.tar.gz','x264-31e19f9.tar.gz']:
    with tarfile.open(src/archive) as t:
        for member in t.getmembers():
            target=(w/'src'/member.name).resolve()
            if not target.is_relative_to(w/'src') or member.issym() or member.islnk():
                raise ValueError('Unsafe source archive member')
        t.extractall(w/'src')
for name, command in [('cc',f'"{zig}" cc -target x86_64-linux-gnu.2.31'),('ar',f'"{zig}" ar'),('ranlib',f'"{zig}" ranlib')]:
    f=w/name; f.write_text('#!/bin/sh\nexec '+command+' "$@"\n'); f.chmod(0o755)
x=w/'src/x264-31e19f92f00c7003fa115047ce50978bc98c3a0d'
xenv=dict(env,CC=str(w/'cc'),AR=str(w/'ar'),RANLIB=str(w/'ranlib'))
subprocess.run(['./configure','--host=x86_64-linux-gnu','--prefix='+str(w/'prefix'),'--enable-static','--disable-cli','--disable-asm','--disable-opencl'],cwd=x,env=xenv,check=True)
subprocess.run(['make','-j6'],cwd=x,env=xenv,check=True)
subprocess.run(['make','install'],cwd=x,env=xenv,check=True)
f=w/'pkg-config'
f.write_text('#!/bin/sh\ncase "$*" in\n*--version*) echo 1.8.0;;\n*--cflags*) echo "-I'+str(w)+'/prefix/include";;\n*--libs*) echo "-L'+str(w)+'/prefix/lib -lx264 -lpthread -lm";;\n*--modversion*) echo 0.164.0;;\n*x264*) exit 0;;\n*) exit 1;;\nesac\n')
f.chmod(0o755)
shutil.copy2(src/'build-ffmpeg.sh',w/'build-ffmpeg.sh')
subprocess.run(['bash',str(w/'build-ffmpeg.sh')],env=env,check=True)
print('Built:',w/'bin/ffmpeg')
print('Add official go2rtc_linux_amd64 v1.9.15 as bin/go2rtc and copy the published licenses directory before building the FPK.')
