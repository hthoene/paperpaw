//! Memory refused while a database is written or a schema read is an
//! error ([`Error::OutOfMemory`]), never an abort: a global allocator here
//! refuses every allocation above a limit while it is armed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_sqlite::{Database, Error, Value, Writer, columns, encode_record, try_encode_record};

mod common;
use common::{Memory, table_entry, text};

/// Largest allocation allowed; `usize::MAX` when not armed.
static LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);

struct Refusing;

// SAFETY: forwards to the system allocator, or refuses with null, which
// `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > LIMIT.load(Ordering::Relaxed) {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout, passed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by `System` above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if size > LIMIT.load(Ordering::Relaxed) {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's block and layout, passed on.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Runs `f` with allocations above `limit` bytes refused. The tests of
/// this file run one at a time (`armed` holds the lock).
fn armed<T>(limit: usize, f: impl FnOnce() -> T) -> T {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    LIMIT.store(limit, Ordering::Relaxed);
    let out = f();
    LIMIT.store(usize::MAX, Ordering::Relaxed);
    out
}

#[test]
fn a_record_too_large_for_memory_is_an_error() {
    let blob = Value::Blob(vec![7; 4000]);
    let values = [Value::Int(1), blob];
    assert_eq!(
        armed(1024, || try_encode_record(&values)),
        Err(Error::OutOfMemory)
    );
    // With room, the same bytes as the infallible form.
    assert_eq!(try_encode_record(&values), Ok(encode_record(&values)));
}

#[test]
fn a_writer_without_room_for_a_page_fails_every_call() {
    let storage = Memory::default();
    let rows: Vec<(i64, Vec<u8>)> = (1..=50)
        .map(|i| (i, encode_record(&[Value::Int(i), text("row")])))
        .collect();
    let schema = [table_entry("t", &["a", "b"], 2)];
    let (table, index, finished) = armed(1024, || {
        let mut writer = Writer::new(&storage, "db");
        let table = writer.table(rows);
        let index = writer.index([encode_record(&[Value::Int(1), Value::Int(1)])]);
        (table, index, writer.finish(&schema, 0))
    });
    assert_eq!(table, Err(Error::OutOfMemory));
    assert_eq!(index, Err(Error::OutOfMemory));
    assert_eq!(finished, Err(Error::OutOfMemory));
    // Nothing is left: no database and no temporary pages.
    assert!(storage.get("db").is_none());
    assert!(storage.get("db.part").is_none());
}

#[test]
fn column_names_without_room_are_an_error() {
    let sql = format!("CREATE TABLE t({})", "column_name_long, ".repeat(40) + "z");
    assert_eq!(armed(16, || columns(&sql)), Err(Error::OutOfMemory));
    assert_eq!(columns(&sql).map(|c| c.len()), Ok(41));
}

#[test]
fn a_database_opened_and_its_table_found_without_room_is_an_error() {
    let storage = Memory::default();
    let root = common::write_table(&storage, &[(1, vec![Value::Int(1), text("a")])]);
    assert!(root > 1);
    let db = Database::open(&storage, "db").expect("opens");
    // The schema's page buffer (4096 bytes) cannot be had.
    assert_eq!(
        armed(1024, || db.table("t").map(|t| t.1)),
        Err(Error::OutOfMemory)
    );
    assert_eq!(
        armed(1024, || Database::open(&storage, "db").map(|_| ())).ok(),
        Some(())
    );
}
