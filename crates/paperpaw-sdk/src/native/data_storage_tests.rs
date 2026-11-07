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
    /// A status every write and append answers with, when not `OK`.
    status: core::cell::Cell<i32>,
    /// The write or append number (from 1) that fails with `ERR_IO`; 0: none.
    fail_at: core::cell::Cell<usize>,
    puts: core::cell::Cell<usize>,
    rename_fails: core::cell::Cell<bool>,
    sync_fails: core::cell::Cell<bool>,
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
    f.puts.set(f.puts.get() + 1);
    if f.puts.get() == f.fail_at.get() {
        return abi::ERR_IO;
    }
    if f.status.get() != OK {
        return f.status.get();
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
    if files(ctx).rename_fails.get() {
        return abi::ERR_IO;
    }
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
unsafe extern "C" fn sync_slot(ctx: *mut c_void) -> i32 {
    if files(ctx).sync_fails.get() {
        return abi::ERR_IO;
    }
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
fn big_appends_and_reads_are_split_into_calls_of_4_kib() {
    with_storage(|storage, files| {
        let data = pattern(MAX_DATA * 2 + 808);
        Storage::append(storage, "log.bin", &data).unwrap();
        Storage::append(storage, "log.bin", &data).unwrap();
        assert_eq!(Storage::size(storage, "log.bin"), Ok(2 * data.len() as u64));
        let mut back = alloc::vec![0; 2 * data.len()];
        assert_eq!(
            Storage::read_at(storage, "log.bin", 0, &mut back),
            Ok(back.len())
        );
        assert_eq!(&back[..data.len()], &data[..]);
        assert_eq!(&back[data.len()..], &data[..]);
        let calls = files.calls.borrow();
        assert!(calls.iter().all(|c| c.1 <= MAX_DATA));
        let appended: usize = calls.iter().filter(|c| c.0 == "append").map(|c| c.1).sum();
        assert_eq!(appended, 2 * data.len());
    });
}

fn has(files: &Files, name: &str) -> bool {
    files.files.borrow().contains_key(name)
}

#[test]
fn a_long_write_is_staged_and_published_whole() {
    with_storage(|storage, files| {
        Storage::write(storage, "books.ini", b"old").unwrap();
        // A stale temporary file of an earlier cut must not leak into the result.
        files
            .files
            .borrow_mut()
            .insert("books.ini.pp-staged".into(), b"STALE".to_vec());
        files.calls.borrow_mut().clear();
        let data = pattern(MAX_DATA * 2 + 5);
        Storage::write(storage, "books.ini", &data).unwrap();
        assert_eq!(*files.files.borrow().get("books.ini").unwrap(), data);
        assert!(!has(files, "books.ini.pp-staged"));
        // Every call carried at most 4 KiB, the first of them to the temporary file.
        assert!(files.calls.borrow().iter().all(|c| c.1 <= MAX_DATA));
        assert_eq!(files.calls.borrow()[0].0, "append");
        // Exactly 4 KiB is one call, in place.
        files.calls.borrow_mut().clear();
        Storage::write(storage, "books.ini", &pattern(MAX_DATA)).unwrap();
        assert_eq!(*files.calls.borrow(), [("write", MAX_DATA)]);
    });
}

#[test]
fn a_failure_on_a_later_piece_keeps_the_old_file_and_removes_the_temporary_one() {
    for fail_at in 1..=3 {
        with_storage(|storage, files| {
            Storage::write(storage, "books.ini", b"old").unwrap();
            files.puts.set(0);
            files.fail_at.set(fail_at);
            assert_eq!(
                Storage::write(storage, "books.ini", &pattern(MAX_DATA * 2 + 5)),
                Err(StorageError::Io)
            );
            assert_eq!(*files.files.borrow().get("books.ini").unwrap(), b"old");
            assert!(!has(files, "books.ini.pp-staged"), "{fail_at}");
            // The next attempt works.
            files.fail_at.set(0);
            Storage::write(storage, "books.ini", &pattern(MAX_DATA + 1)).unwrap();
            assert_eq!(
                files.files.borrow().get("books.ini").unwrap().len(),
                MAX_DATA + 1
            );
        });
    }
}

#[test]
fn a_failed_replacement_keeps_the_old_file_and_cleans_up() {
    with_storage(|storage, files| {
        Storage::write(storage, "books.ini", b"old").unwrap();
        files.rename_fails.set(true);
        assert_eq!(
            Storage::write(storage, "books.ini", &pattern(MAX_DATA + 1)),
            Err(StorageError::Io)
        );
        assert_eq!(*files.files.borrow().get("books.ini").unwrap(), b"old");
        assert!(!has(files, "books.ini.pp-staged"));
    });
}

#[test]
fn a_name_that_leaves_no_room_for_the_temporary_one_is_refused_untouched() {
    with_storage(|storage, files| {
        let long = alloc::format!(
            "{}/{}/{}",
            "a".repeat(200),
            "b".repeat(200),
            "c".repeat(110)
        );
        assert_eq!(long.len(), MAX_PATH);
        assert_eq!(
            Storage::write(storage, &long, &pattern(MAX_DATA + 1)),
            Err(StorageError::InvalidPath)
        );
        assert!(files.calls.borrow().is_empty());
    });
}

#[test]
fn running_out_of_memory_is_not_taken_for_the_card_taken_out() {
    with_storage(|storage, files| {
        files.status.set(abi::ERR_NO_MEMORY);
        let refused = Storage::append(storage, "a", b"x").unwrap_err();
        assert_eq!(refused, StorageError::Io);
        assert_ne!(refused, StorageError::CARD_GONE);
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
            Storage::size(storage, "../x"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(
            Storage::size(storage, &"a".repeat(MAX_PATH + 1)),
            Err(StorageError::Full)
        );
    });
}

#[test]
fn the_staging_suffix_is_reserved_in_both_layers_so_no_other_file_is_in_the_way() {
    let table = table();
    let mut files = Files::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut files).cast(),
    };
    // SAFETY: the context is only read through shared references from here on.
    let files = unsafe { &*ptr::from_ref(&files) };
    let storage = DataStorage::new(&host);
    for name in ["a.pp-staged", "a.PP-STAGED", "dir/b.Pp-Staged"] {
        // The free functions of the module ...
        assert_eq!(write(&host, Root::Data, name, b"x"), Err(Error::Invalid));
        assert_eq!(append(&host, Root::Data, name, b"x"), Err(Error::Invalid));
        assert_eq!(remove(&host, Root::Data, name), Err(Error::Invalid));
        assert_eq!(rename(&host, Root::Data, name, "z"), Err(Error::Invalid));
        assert_eq!(rename(&host, Root::Data, "z", name), Err(Error::Invalid));
        // ... and the Storage on top of them, short or long.
        let invalid = Err(StorageError::InvalidPath);
        assert_eq!(Storage::write(&storage, name, b"x"), invalid);
        assert_eq!(
            Storage::write(&storage, name, &pattern(MAX_DATA + 1)),
            invalid
        );
        assert_eq!(Storage::append(&storage, name, b"x"), invalid);
        assert_eq!(Storage::remove(&storage, name), invalid);
        assert_eq!(Storage::rename(&storage, name, "z"), invalid);
        assert_eq!(Storage::rename(&storage, "z", name), invalid);
    }
    assert!(files.calls.borrow().is_empty());
    // Reading is allowed; names that merely contain it are ordinary.
    assert_eq!(size(&host, Root::Data, "a.pp-staged"), Err(Error::NotFound));
    Storage::write(&storage, "a.pp-staged.bak", b"x").unwrap();
    Storage::write(&storage, "pp-staged", b"x").unwrap();
}

#[test]
fn an_unrelated_name_tmp_survives_a_staged_write_whether_it_works_or_not() {
    with_storage(|storage, files| {
        Storage::write(storage, "books.ini", b"old").unwrap();
        Storage::write(storage, "books.ini.tmp", b"somebody's").unwrap();
        Storage::write(storage, "books.ini.bak", b"keep").unwrap();
        let big = pattern(MAX_DATA + 1);
        let intact = |files: &Files| {
            assert_eq!(
                *files.files.borrow().get("books.ini.tmp").unwrap(),
                b"somebody's"
            );
            assert_eq!(*files.files.borrow().get("books.ini.bak").unwrap(), b"keep");
            assert!(!has(files, "books.ini.pp-staged"));
        };
        // A failure on a piece, at the sync, and at the rename ...
        files.puts.set(0);
        files.fail_at.set(2);
        assert!(Storage::write(storage, "books.ini", &big).is_err());
        files.fail_at.set(0);
        files.sync_fails.set(true);
        assert!(Storage::write(storage, "books.ini", &big).is_err());
        files.sync_fails.set(false);
        files.rename_fails.set(true);
        assert!(Storage::write(storage, "books.ini", &big).is_err());
        files.rename_fails.set(false);
        assert_eq!(*files.files.borrow().get("books.ini").unwrap(), b"old");
        intact(files);
        // ... and a success.
        Storage::write(storage, "books.ini", &big).unwrap();
        assert_eq!(*files.files.borrow().get("books.ini").unwrap(), big);
        intact(files);
    });
}
