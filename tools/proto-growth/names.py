import json,re,collections,subprocess,sys
rows=json.load(open('cands.json'))
repo='/workspaces/paperpaw/firmware-engineer/paperpaw'
# public fns from api.txt: key (crate, ty or None, name) -> list of full lines
api=collections.defaultdict(list)
for f in subprocess.run(['bash','-c',f'ls {repo}/crates/*/api.txt'],capture_output=True,text=True).stdout.split():
    for line in open(f):
        m=re.match(r'^pub (?:unsafe |const |async )*fn (paperpaw_\w+)::(.*?)\((.*)$',line)
        if not m: continue
        crate,path=m[1],m[2]
        # path may contain generics before '(': split off '<..>' on last segment
        segs=re.split(r'::',path)
        name=re.sub(r'<.*$','',segs[-1]) if True else segs[-1]
        ty=segs[-2] if len(segs)>=2 else None
        ty=re.sub(r'<.*$','',ty) if ty else None
        api[(crate,ty,name)].append(line.strip())
def key(d):
    m=re.match(r'^<([^<>]*?(?:<[^<>]*>)?[^<>]*?)(?: as ([^>]*))?>::(\w+)$',d)
    if m:
        ty=m[1].split('::')[-1]; ty=re.sub(r'<.*$','',ty); crate=m[1].split('::')[0]
        return (crate,ty,m[3],'trait' if m[2] else 'inherent', m[2])
    m=re.match(r'^(\w+)::(.*)::(\w+)$',d)
    if m:
        segs=d.split('::'); return (segs[0],segs[-2] if False else None,segs[-1],'free',None)
    m=re.match(r'^(\w+)::(\w+)$',d)
    if m: return (m[1],None,m[2],'free',None)
    return None
cat=collections.Counter(); matched=[]; unmatched=[]
for d,r in rows.items():
    k=key(d)
    if not k: cat['unparsed']+=1; unmatched.append((d,r,'unparsed')); continue
    kind=k[3]
    hit=api.get((k[0],k[1],k[2]),[])
    if kind=='free' and not hit:
        # free fn: type None, but api key has ty = module name; search any ty
        hit=[l for (c,t,n),ls in api.items() if c==k[0] and n==k[2] for l in ls if t is not None]
    cat[(kind,'public' if hit else 'nonpublic')]+=1
    (matched if hit else unmatched).append((d,r,kind))
print(cat)
json.dump({'matched':[(d,r) for d,r,k in matched],'unmatched':[(d,r,k) for d,r,k in unmatched]},open('cat.json','w'))
