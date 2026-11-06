#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{string::String, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, OK, PpHost};
#[derive(Debug, PartialEq, Eq)]
struct Call(&'static str, u32, String, Vec<u8>, u64);
#[derive(Default)]
struct Context {
    calls: RefCell<Vec<Call>>,
    result: Cell<i32>,
    count: Cell<i32>,
}
unsafe fn log<'a>(
    ctx: *mut c_void,
    op: &'static str,
    root: u32,
    path: PpBytes,
    data: PpBytes,
    offset: u64,
) -> &'a Context {
    // SAFETY: test tables receive live Context and readable path/data slices.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let path = core::str::from_utf8(core::slice::from_raw_parts(path.ptr, path.len)).unwrap();
        let data = core::slice::from_raw_parts(data.ptr, data.len);
        ctx.calls
            .borrow_mut()
            .push(Call(op, root, path.into(), data.into(), offset));
        ctx
    }
}
fn empty() -> PpBytes {
    bytes(b"")
}
unsafe extern "C" fn size_slot(ctx: *mut c_void, root: u32, path: PpBytes, out: *mut u64) -> i32 {
    // SAFETY: wrapper supplies live inputs and writable output.
    unsafe {
        let ctx = log(ctx, "size", root, path, empty(), 0);
        if ctx.result.get() == OK {
            out.write(u64::MAX);
        }
        ctx.result.get()
    }
}
unsafe extern "C" fn read(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    // SAFETY: wrapper supplies exclusively borrowed output and live inputs.
    unsafe {
        let ctx = log(ctx, "read", root, path, empty(), offset);
        if ctx.result.get() != OK {
            return ctx.result.get();
        }
        let n = ctx.count.get();
        if let Ok(n) = usize::try_from(n)
            && n <= out.cap
        {
            ptr::write_bytes(out.ptr, 42, n);
        }
        n
    }
}
unsafe extern "C" fn write_slot(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    // SAFETY: table inputs come from the safe wrapper for this call.
    unsafe { log(ctx, "write", root, path, data, 0).result.get() }
}
unsafe extern "C" fn append_slot(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    // SAFETY: table inputs come from the safe wrapper for this call.
    unsafe { log(ctx, "append", root, path, data, 0).result.get() }
}
unsafe extern "C" fn rename_slot(ctx: *mut c_void, root: u32, from: PpBytes, to: PpBytes) -> i32 {
    // SAFETY: table inputs come from the safe wrapper for this call.
    unsafe { log(ctx, "rename", root, from, to, 0).result.get() }
}
unsafe extern "C" fn remove_slot(ctx: *mut c_void, root: u32, path: PpBytes) -> i32 {
    // SAFETY: table inputs come from the safe wrapper for this call.
    unsafe { log(ctx, "remove", root, path, empty(), 0).result.get() }
}
unsafe extern "C" fn sync_slot(ctx: *mut c_void) -> i32 {
    // SAFETY: table context comes from the safe wrapper for this call.
    unsafe { log(ctx, "sync", 0, empty(), empty(), 0).result.get() }
}
fn table() -> PpHost {
    let mut t = abi::host_table();
    t.storage_size = size_slot;
    t.storage_read_at = read;
    t.storage_write = write_slot;
    t.storage_append = append_slot;
    t.storage_rename = rename_slot;
    t.storage_remove = remove_slot;
    t.storage_sync = sync_slot;
    t
}

#[test]
fn every_slot_forwards_root_paths_buffers_and_status() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    assert_eq!(size(&host, 0, "log.csv"), Ok(u64::MAX));
    ctx.count.set(2);
    let mut out = [0; 5];
    assert_eq!(read_at(&host, 1, "book.epub", u64::MAX, &mut out), Ok(2));
    assert_eq!(out, [42, 42, 0, 0, 0]);
    write(&host, 0, ".hidden/a", b"new").unwrap();
    append(&host, 0, "log.csv", b"").unwrap();
    rename(&host, 0, "new", "old").unwrap();
    remove(&host, 0, "old").unwrap();
    sync(&host).unwrap();
    assert_eq!(
        *ctx.calls.borrow(),
        vec![
            Call("size", 0, "log.csv".into(), vec![], 0),
            Call("read", 1, "book.epub".into(), vec![], u64::MAX),
            Call("write", 0, ".hidden/a".into(), b"new".into(), 0),
            Call("append", 0, "log.csv".into(), vec![], 0),
            Call("rename", 0, "new".into(), b"old".into(), 0),
            Call("remove", 0, "old".into(), vec![], 0),
            Call("sync", 0, "".into(), vec![], 0),
        ]
    );
    ctx.result.set(abi::ERR_FULL);
    assert_eq!(append(&host, 0, "log", b"x"), Err(Error::Full));
    ctx.result.set(abi::ERR_IO);
    assert_eq!(sync(&host), Err(Error::Io));
    ctx.result.set(abi::ERR_NOT_FOUND);
    assert_eq!(size(&host, 0, "missing"), Err(Error::NotFound));
}

#[test]
fn invalid_input_is_refused_before_any_host_call() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    for path in [
        "", "/x", "x/", "a//b", "..", "a/../b", "a\\b", "a:b", "NUL", "a.", "a ", "a\n",
    ] {
        assert_eq!(size(&host, 0, path), Err(Error::Invalid));
        assert_eq!(rename(&host, 0, "valid", path), Err(Error::Invalid));
    }
    assert_eq!(size(&host, 0, "a".repeat(256)), Err(Error::NoSpace));
    assert_eq!(size(&host, 0, "a/".repeat(257)), Err(Error::NoSpace));
    assert_eq!(size(&host, 0, "é".repeat(128)), Err(Error::NoSpace));
    assert_eq!(
        append(&host, 0, "a", &[0; MAX_DATA + 1]),
        Err(Error::NoSpace)
    );
    assert_eq!(
        write(&host, 0, "a", &[0; MAX_DATA + 1]),
        Err(Error::NoSpace)
    );
    assert_eq!(
        read_at(&host, 0, "a", 0, &mut [0; MAX_DATA + 1]),
        Err(Error::NoSpace)
    );
    assert!(ctx.calls.borrow().is_empty());
}

#[test]
fn caps_empty_buffers_eof_and_invalid_host_counts() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let path = alloc::format!("{}/{}/", "a".repeat(255), "b".repeat(254));
    let path = alloc::format!("{}c", &path[..511]);
    assert_eq!(path.len(), MAX_PATH);
    assert_eq!(size(&host, 7, path), Ok(u64::MAX));
    write(&host, 0, "a", &[1; MAX_DATA]).unwrap();
    write(&host, 0, "a", b"").unwrap();
    assert_eq!(read_at(&host, 0, "a", 0, &mut []), Ok(0));
    assert_eq!(read_at(&host, 0, "a", 0, &mut [0; 3]), Ok(0));
    ctx.count.set(4);
    assert_eq!(read_at(&host, 0, "a", 0, &mut [0; 3]), Err(Error::Invalid));
    ctx.count.set(abi::ERR_STATE);
    assert_eq!(read_at(&host, 0, "a", 0, &mut [0; 3]), Err(Error::State));
}

#[test]
fn default_table_refuses_all_storage_operations() {
    let table = abi::host_table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    assert_eq!(size(&host, 0, "x"), Err(Error::Unsupported));
    assert_eq!(
        read_at(&host, 0, "x", 0, &mut [0; 1]),
        Err(Error::Unsupported)
    );
    assert_eq!(write(&host, 0, "x", b"a"), Err(Error::Unsupported));
    assert_eq!(append(&host, 0, "x", b"a"), Err(Error::Unsupported));
    assert_eq!(rename(&host, 0, "x", "y"), Err(Error::Unsupported));
    assert_eq!(remove(&host, 0, "x"), Err(Error::Unsupported));
    assert_eq!(sync(&host), Err(Error::Unsupported));
}
