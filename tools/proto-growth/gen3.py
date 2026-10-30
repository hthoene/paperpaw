import json,re,subprocess,sys
repo='/workspaces/paperpaw/firmware-engineer/paperpaw'
ver=json.load(open('/home/agent/fw-jobs/proto/table_ver.json'))
subprocess.run(['python3','gen_table2.py','x','prune'],capture_output=True)
stress=json.load(open('/home/agent/fw-jobs/proto/table_entries2.json'))
drop=set(json.load(open('/home/agent/fw-jobs/proto/drop3.json'))) if len(sys.argv)>1 else set()
stress=[e for e in stress if e[0] not in drop and e[0] not in set(x[0] for x in ver)]
def arr(name,sel):
    L=[f'pub static {name}: Table<{len(sel)}> = Table([']
    for e,d,s in sel: L.append(f'    {e} as *const (), // {s} B')
    L.append(']);'); return '\n'.join(L)
src='''//! EXPERIMENT ONLY (proto/shared-table, #44): addresses of public non-generic functions as a plain table.
#![allow(clippy::all, unused)]
pub struct Table<const N: usize>(pub [*const (); N]);
// SAFETY: addresses of functions, never written.
unsafe impl<const N: usize> Sync for Table<N> {}

#[cfg(feature = "proto-ver")]
'''+arr('TABLE',ver)+'''

#[cfg(feature = "proto-stress")]
'''+arr('TABLE',ver+stress)+'''

pub fn touch(n: u64) -> usize {
    let table = core::hint::black_box(&TABLE.0);
    // SAFETY: within the table.
    unsafe { core::ptr::read_volatile(table.as_ptr().add((n as usize) % table.len())) as usize }
}
'''
open(repo+'/shells/xteink/src/proto_table.rs','w').write(src)
print(len(ver),len(stress))
