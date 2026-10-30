import sys,re,glob,json,collections
sys.path.insert(0,'/home/agent/fw-jobs/proto'); import syms
fw={}
for a,s,k,m,d in syms.load(sys.argv[1]):
    if k in 'tT': fw[d]=fw.get(d,0)+s
def crate(d):
    s=d.lstrip('<&'); m=re.match(r'([A-Za-z_][A-Za-z0-9_]*)::',s); return m[1] if m else ''
def generic(d): return '::<' in d or '{closure' in d or '{shim' in d or '{impl' in d or '{constant' in d
rows=collections.defaultdict(lambda:{'p':0,'bytes':0,'size':0})
nplug=0
for f in sorted(glob.glob('plugin-base/*')):
    if f.endswith('.d'): continue
    nplug+=1; seen=set()
    for a,s,k,m,d in syms.load(f):
        if k not in 'tT' or s<64: continue
        c=crate(d)
        if not c.startswith('paperpaw_') or c.startswith('paperpaw_plugin_') or generic(d) or d in seen: continue
        seen.add(d); r=rows[d]; r['p']+=1; r['bytes']+=s; r['size']=max(r['size'],s)
STUB=12; TAB=4
res=[]
for d,r in rows.items():
    infw = d in fw
    growth = TAB if infw else r['size']+TAB
    save = r['bytes']-STUB*r['p']
    res.append((save-growth,d,r['p'],r['size'],infw,save,growth))
res.sort(reverse=True)
def summ(sel,label):
    n=len(sel); save=sum(x[5] for x in sel); gr=sum(x[6] for x in sel)
    print(f'{label:40s} entries {n:4d}  plugin saving {save/1024:7.0f} KiB  firmware growth {gr/1024:6.1f} KiB  net {(save-gr)/1024:7.0f} KiB')
pos=[x for x in res if x[0]>0]
summ(res,'all non-generic paperpaw fns >=64B')
summ([x for x in res if x[4]],'  of which with a twin in fw')
summ([x for x in res if not x[4]],'  of which fw has no out-of-line copy')
summ(pos,'net gain > 0 (chosen by net gain)')
summ([x for x in pos if x[2]>=2],'net gain > 0 and in >= 2 plugins')
summ([x for x in res if x[3]>=256],'size >= 256 B (the study rule)')
summ([x for x in res if x[3]>=256 and x[4]],'size >= 256 B with fw twin')
for n in (50,100,200,300):
    summ(res[:n],f'top {n} by net gain')
print('plugins',nplug)
json.dump(res,open('netgain.json','w'))
for x in res[:15]: print(x[0],x[2],x[3],x[4],x[1][:90])
print('worst:')
for x in res[-5:]: print(x[0],x[2],x[3],x[4],x[1][:90])
