//! `DataStorage` over a host that keeps files in memory and refuses what the
//! real one refuses: more than `MAX_DATA` in a call, and any root but 0.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::{cell::RefCell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self as abi, OK, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct Files {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    calls: RefCell<Vec<(&'static str, usize)>>,
    full: core::cell::Cell<bool>,
}

fn name(path: PpBytes) -> String {
    // SAFETY: the wrapper passes a live path slice.
    let bytes = unsafe { core::slice::from_raw_parts(path.ptr, path.len) };
    String::from(core::str::from_utf8(bytes).unwrap())
}
fn files<'a>(ctx: *mut c_void) -> &'a Files {
    // SAFETY: the fixture context outlives the call.
    unsafe { &*ctx.cast::<Files>() }
}

unsafe extern "C" fn size_slot(ctx: *mut c_void, root: u32, path: PpBytes, out: *mut u64) -> i32 {
    assert_eq!(root, 0);
    match files(ctx).files.borrow().get(&name(path)) {
        Some(data) => {
            // SAFETY: the wrapper passes a writable output.
            unsafe { out.write(data.len() as u64) };
            OK
        }
        None => abi::ERR_NOT_FOUND,
    }
}
unsafe extern "C" fn read_slot(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    assert_eq!(root, 0);
    assert!(out.cap <= MAX_DATA);
    files(ctx).calls.borrow_mut().push(("read", out.cap));
    let map = files(ctx).files.borrow();
    let Some(data) = map.get(&name(path)) else {
        return abi::ERR_NOT_FOUND;
    };
    let from = usize::try_from(offset).unwrap().min(data.len());
    let n = (data.len() - from).min(out.cap);
    // SAFETY: `out` is exclusively ours for `cap` bytes.
    unsafe { ptr::copy_nonoverlapping(data[from..].as_ptr(), out.ptr, n) };
    i32::try_from(n).unwrap()
}
fn put(ctx: *mut c_void, op: &'static str, path: PpBytes, data: PpBytes, replace: bool) -> i32 {
    assert!(data.len <= MAX_DATA);
    let f = files(ctx);
    f.calls.borrow_mut().push((op, data.len));
    if f.full.get() {
        return abi::ERR_FULL;
    }
    // SAFETY: the wrapper passes a live data slice.
    let bytes = unsafe { core::slice::from_raw_parts(data.ptr, data.len) };
    let mut map = f.files.borrow_mut();
    let file = map.entry(name(path)).or_default();
    if replace {
        file.clear();
    }
    file.extend_from_slice(bytes);
    OK
}
unsafe extern "C" fn write_slot(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    assert_eq!(root, 0);
    put(ctx, "write", path, data, true)
}
unsafe extern "C" fn append_slot(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    assert_eq!(root, 0);
    put(ctx, "append", path, data, false)
}
unsafe extern "C" fn rename_slot(ctx: *mut c_void, root: u32, from: PpBytes, to: PpBytes) -> i32 {
    assert_eq!(root, 0);
    let mut map = files(ctx).files.borrow_mut();
    match map.remove(&name(from)) {
        Some(data) => {
            map.insert(name(to), data);
            OK
        }
        None => abi::ERR_NOT_FOUND,
    }
}
unsafe extern "C" fn remove_slot(ctx: *mut c_void, root: u32, path: PpBytes) -> i32 {
    assert_eq!(root, 0);
    match files(ctx).files.borrow_mut().remove(&name(path)) {
        Some(_) => OK,
        None => abi::ERR_NOT_FOUND,
    }
}
unsafe extern "C" fn sync_slot(_: *mut c_void) -> i32 {
    OK
}

fn table() -> PpHost {
    let mut t = abi::host_table();
    t.storage_size = size_slot;
    t.storage_read_at = read_slot;
    t.storage_write = write_slot;
    t.storage_append = append_slot;
    t.storage_rename = rename_slot;
    t.storage_remove = remove_slot;
    t.storage_sync = sync_slot;
    t
}

fn with_storage<T>(run: impl FnOnce(&DataStorage<'_, '_>, &Files) -> T) -> T {
    let table = table();
    let mut files = Files::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut files).cast(),
    };
    // SAFETY: the context is only read through shared references from here on.
    let shared = unsafe { &*ptr::from_ref(&files) };
    run(&DataStorage::new(&host), shared)
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[test]
fn big_writes_appends_and_reads_are_split_into_calls_of_4_kib() {
    with_storage(|storage, files| {
        let data = pattern(MAX_DATA * 2 + 808);
        Storage::write(storage, "log.bin", &data).unwrap();
        assert_eq!(Storage::size(storage, "log.bin"), Ok(data.len() as u64));
        Storage::append(storage, "log.bin", &data).unwrap();
        assert_eq!(Storage::size(storage, "log.bin"), Ok(2 * data.len() as u64));
        let mut back = alloc::vec![0; 2 * data.len()];
        assert_eq!(
            Storage::read_at(storage, "log.bin", 0, &mut back),
            Ok(back.len())
        );
        assert_eq!(&back[..data.len()], &data[..]);
        assert_eq!(&back[data.len()..], &data[..]);
        // The first piece replaced the file, the rest was appended.
        let calls = files.calls.borrow();
        let writes: Vec<_> = calls.iter().filter(|c| c.0 != "read").collect();
        assert_eq!(writes[0], &("write", MAX_DATA));
        assert!(writes[1..].iter().all(|c| c.0 == "append"));
        assert_eq!(writes.iter().map(|c| c.1).sum::<usize>(), 2 * data.len());
    });
}

#[test]
fn a_short_read_ends_the_read_and_the_end_of_a_file_is_zero() {
    with_storage(|storage, _| {
        Storage::write(storage, "a", b"hello").unwrap();
        let mut buf = [0; 10_000];
        assert_eq!(Storage::read_at(storage, "a", 0, &mut buf), Ok(5));
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(Storage::read_at(storage, "a", 5, &mut buf), Ok(0));
        assert_eq!(Storage::read_at(storage, "a", u64::MAX, &mut buf), Ok(0));
        assert_eq!(
            Storage::read_at(storage, "missing", 0, &mut buf),
            Err(StorageError::NotFound)
        );
    });
}

#[test]
fn removing_a_missing_file_is_fine_renaming_moves_and_empty_append_creates() {
    with_storage(|storage, _| {
        assert_eq!(Storage::remove(storage, "none"), Ok(()));
        Storage::append(storage, "new", &[]).unwrap();
        assert_eq!(Storage::size(storage, "new"), Ok(0));
        Storage::write(storage, "x", b"1").unwrap();
        Storage::rename(storage, "x", "y").unwrap();
        assert!(!Storage::exists(storage, "x") && Storage::exists(storage, "y"));
        assert_eq!(
            Storage::rename(storage, "x", "z"),
            Err(StorageError::NotFound)
        );
        Storage::remove(storage, "y").unwrap();
        assert!(!Storage::exists(storage, "y"));
        Storage::sync(storage).unwrap();
        assert_eq!(
            Storage::list(storage, "", &mut |_| {}),
            Err(StorageError::Unsupported)
        );
    });
}

#[test]
fn a_full_card_and_a_bad_path_come_back_as_storage_errors() {
    with_storage(|storage, files| {
        files.full.set(true);
        assert_eq!(Storage::append(storage, "a", b"x"), Err(StorageError::Full));
        assert_eq!(
            Storage::write(storage, "a", &pattern(MAX_DATA + 1)),
            Err(StorageError::Full)
        );
        assert_eq!(
            Storage::size(storage, "../x"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(
            Storage::size(storage, &"a".repeat(MAX_PATH + 1)),
            Err(StorageError::Full)
        );
    });
}
