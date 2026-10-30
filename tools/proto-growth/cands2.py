#!/usr/bin/env python3
"""Candidates for the table: symbols (demangled) in >=1 plugin ELF that also exist in the firmware ELF with the
same name, are non-generic, belong to a PaperPaw crate, and have size >= MIN in the plugin."""
import sys, re, glob, json, collections
sys.path.insert(0,'/home/agent/fw-jobs/proto')
import syms
fw = syms.load(sys.argv[1])
fwn = collections.defaultdict(list)
for a,s,k,m,d in fw:
    if k in 'tT': fwn[d].append((s,m))
plug = {}
for f in sorted(glob.glob('/home/agent/fw-jobs/proto/plugin-base/*')):
    if f.endswith('.d'): continue
    plug[f.split('/')[-1]] = syms.load(f)
def crate(d):
    s=d.lstrip('<&'); m=re.match(r'([A-Za-z_][A-Za-z0-9_]*)::',s); return m[1] if m else ''
def generic(d):
    return '::<' in d or '{closure' in d or '{shim' in d or '{impl' in d or '{constant' in d or 'dyn ' in d and False
rows = collections.defaultdict(lambda: {'plugins':0,'bytes':0,'size':0,'fw':0})
for p,ss in plug.items():
    seen=set()
    for a,s,k,m,d in ss:
        if k not in 'tT' or s<64: continue
        c=crate(d)
        if not c.startswith('paperpaw_') or c.startswith('paperpaw_plugin_'): continue
        if generic(d) or d in seen: continue
        if d in fwn: continue
        seen.add(d)
        r=rows[d]; r['plugins']+=1; r['bytes']+=s; r['size']=max(r['size'],s); r['fw']=0
json.dump(rows, open('cands2.json','w'))
for MIN in (64,256):
    sel={d:r for d,r in rows.items() if r['size']>=MIN}
    print(MIN, len(sel), sum(r['bytes'] for r in sel.values()))
byc=collections.Counter(); 
for d,r in rows.items(): byc[crate(d)]+=1
print(byc.most_common(20))
