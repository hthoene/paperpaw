#!/usr/bin/env python3
"""nm -S (mangled and demangled) of an ELF -> list of (addr,size,kind,mangled,demangled) for sized symbols."""
import subprocess, sys, re
R = re.compile(r'^([0-9a-f]+) ([0-9a-f]+) (\w) (.*)$')
def load(elf):
    a = subprocess.run(['nm','-S',elf],capture_output=True,text=True).stdout.splitlines()
    b = subprocess.run(['nm','-S','-C',elf],capture_output=True,text=True).stdout.splitlines()
    assert len(a)==len(b)
    out=[]
    for x,y in zip(a,b):
        m=R.match(x); n=R.match(y)
        if not m: continue
        out.append((int(m[1],16),int(m[2],16),m[3],m[4],n[4]))
    return out
