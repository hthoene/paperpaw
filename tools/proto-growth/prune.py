import subprocess,re,json,os,sys
repo='/workspaces/paperpaw/firmware-engineer/paperpaw'
drop=set()
json.dump([],open('drop.json','w'))
for it in range(12):
    subprocess.run(['python3','gen_table.py','x','prune'],check=True,capture_output=True)
    src=open(repo+'/shells/xteink/src/proto_table.rs').read().splitlines()
    r=subprocess.run(['bash','-c','. ~/.cargo/env; cd '+repo+'/shells/xteink && cargo check --release --target riscv32imc-unknown-none-elf --no-default-features --features x4,proto-all --message-format=short 2>&1'],capture_output=True,text=True)
    errs=set(int(m[1]) for m in re.finditer(r'proto_table\.rs:(\d+):\d+: error',r.stdout))
    other=[l for l in r.stdout.splitlines() if 'error' in l and 'proto_table' not in l]
    print('iter',it,'errors in table:',len(errs),'other:',other[:3])
    if not errs: 
        if other: print(r.stdout[-3000:])
        break
    for ln in errs:
        e=src[ln-1].strip().split(' as usize')[0]
        drop.add(e)
    json.dump(sorted(drop),open('drop.json','w'))
print('dropped',len(drop))
