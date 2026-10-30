#!/usr/bin/env python3
"""growth.py <base.elf> <table.elf> <entries.json>: size delta by section; table verification (each entry
resolves to the symbol of the intended name); symbols new or grown relative to base."""
import sys,struct,json,re,collections
sys.path.insert(0,'/home/agent/fw-jobs/proto'); import syms
def sections(path):
    d=open(path,'rb').read()
    shoff,=struct.unpack_from('<I',d,0x20); shentsize,shnum,shstrndx=struct.unpack_from('<HHH',d,0x2e)
    secs=[]
    for i in range(shnum):
        n,t,f,addr,off,size=struct.unpack_from('<IIIIII',d,shoff+i*shentsize); secs.append([n,t,f,addr,off,size])
    stroff=secs[shstrndx][4]
    out=[]
    for n,t,f,addr,off,size in secs:
        e=d.index(b'\0',stroff+n); out.append((d[stroff+n:e].decode(),t,addr,off,size))
    return d,out
def read_words(path,addr,count):
    d,secs=sections(path)
    for name,t,a,off,size in secs:
        if t!=8 and a<=addr<a+size and size:
            o=off+addr-a; return struct.unpack_from('<%dI'%count,d,o)
base,tab,entries=sys.argv[1:4]
def sz(path):
    d,secs=sections(path); return {n:s for n,t,a,o,s in secs}
b,t=sz(base),sz(tab)
keys=['.text','.rodata','.rodata.wifi','.rwtext','.rwtext.wifi','.trap','.data','.data.wifi','.flash.appdesc']
print('section deltas:',{k:t.get(k,0)-b.get(k,0) for k in keys if t.get(k,0)-b.get(k,0)})
print('text+rodata delta:',t['.text']+t['.rodata']-b['.text']-b['.rodata'],' image-ish delta:',sum(t.get(k,0)-b.get(k,0) for k in keys))
S=syms.load(tab); B=syms.load(base)
tsym=[r for r in S if r[4].endswith('proto_table::TABLE')]
print('table symbol:',[(hex(r[0]),r[1]) for r in tsym])
ents=json.load(open(entries))
if len(sys.argv)>4: ents=[e for e in ents if e[2]>=256]
n=len(ents)
words=read_words(tab,tsym[0][0],n)
# map address -> symbol (text)
text=sorted((a,s,m,d) for a,s,k,m,d in S if k in 'tT' and s>0)
import bisect
starts=[x[0] for x in text]
ok=0; bad=[]; names=collections.defaultdict(list)
for (e,d,s),w in zip(ents,words):
    w&=~1
    i=bisect.bisect_right(starts,w)-1
    a,sz_,m,dn=text[i]
    dem=dn
    if a==w and (dem==d or dem.startswith(d)) : ok+=1
    else: bad.append((e,d,hex(w),dem[:90], a==w))
print('entries',n,'resolved to the intended symbol',ok,'other',len(bad))
for x in bad[:12]: print('  ',x)
# symbol diff
bt=collections.defaultdict(int)
for a,s,k,m,d in B:
    if k in 'tT': bt[d]+=s
tt=collections.defaultdict(int)
for a,s,k,m,d in S:
    if k in 'tT': tt[d]+=s
new=[(s,d) for d,s in tt.items() if d not in bt]
grown=[(tt[d]-bt[d],d) for d in tt if d in bt and tt[d]!=bt[d]]
gone=[(bt[d],d) for d in bt if d not in tt]
print('symbols new:',len(new),sum(s for s,d in new),'B; changed:',len(grown),sum(x for x,d in grown),'B; gone:',len(gone),sum(s for s,d in gone),'B')
for s,d in sorted(new,reverse=True)[:12]: print('  new',s,d[:100])
for s,d in sorted(grown,reverse=True)[:6]: print('  grown',s,d[:100])
for s,d in sorted(gone,reverse=True)[:6]: print('  gone',s,d[:100])
json.dump({'bad':bad,'new':new,'grown':grown,'gone':gone},open(sys.argv[2]+'.growth.json','w'))
