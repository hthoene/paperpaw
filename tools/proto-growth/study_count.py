import sys,re,glob,collections
sys.path.insert(0,'/home/agent/fw-jobs/proto'); import syms
fw=set(d for a,s,k,m,d in syms.load(sys.argv[1]) if k in 'tT' and not d.startswith('.L'))
def crate(d):
    s=d.lstrip('<&'); m=re.match(r'([A-Za-z_][A-Za-z0-9_]*)::',s); return m[1] if m else '?'
dist={}
for f in glob.glob('plugin-base/*'):
    if f.endswith('.d'): continue
    for a,s,k,m,d in syms.load(f):
        if k not in 'tT' or d not in fw or crate(d).startswith('paperpaw_plugin_'): continue
        dist[d]=max(dist.get(d,0),s)
for MIN in (64,256):
    sel=[d for d,s in dist.items() if s>=MIN]
    c=collections.Counter()
    for d in sel:
        cr=crate(d)
        g='::<' in d or '{closure' in d
        c[('paperpaw' if cr.startswith('paperpaw_') else 'core/alloc/other', 'generic' if g else 'plain')]+=1
    print(MIN,len(sel),dict(c))
