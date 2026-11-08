//! Memory refused while a database is written or a schema read is an
//! error ([`Error::OutOfMemory`]), never an abort: the global allocator
//! here refuses allocations while a test thread has armed it. Arming is
//! per thread and undone on drop, so other tests (and libtest itself)
//! running beside one never see a refusal.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sqlite::{Database, Error, Value, Writer, columns, encode_record, try_encode_record};

mod common;
use common::{Memory, index_entry, table_entry, text};

/// This thread's refusals: allocations above `limit` bytes are refused
/// once `left` of them have been allowed; `refused` counts refusals.
struct Arm {
    limit: Cell<usize>,
    left: Cell<usize>,
    refused: Cell<usize>,
}

thread_local! {
    static ARM: Arm = const {
        Arm {
            limit: Cell::new(usize::MAX),
            left: Cell::new(0),
            refused: Cell::new(0),
        }
    };
}

/// Whether this thread may allocate `size` bytes now.
fn allowed(size: usize) -> bool {
    ARM.try_with(|arm| {
        if size <= arm.limit.get() {
            return true;
        }
        match arm.left.get() {
            0 => {
                arm.refused.set(arm.refused.get() + 1);
                false
            }
            left => {
                arm.left.set(left - 1);
                true
            }
        }
    })
    .unwrap_or(true)
}

struct Refusing;

// SAFETY: forwards to the system allocator, or refuses with null, which
// `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !allowed(layout.size()) {
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
        if !allowed(size) {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's block and layout, passed on.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// This thread's refusals until dropped; then what was there before
/// (also when a test panics).
struct Armed {
    limit: usize,
    left: usize,
}

impl Armed {
    fn new(limit: usize, left: usize) -> Self {
        ARM.with(|arm| Self {
            limit: arm.limit.replace(limit),
            left: arm.left.replace(left),
        })
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        ARM.with(|arm| {
            arm.limit.set(self.limit);
            arm.left.set(self.left);
        });
    }
}

/// Runs `f` with this thread's allocations above `limit` bytes refused.
fn armed<T>(limit: usize, f: impl FnOnce() -> T) -> T {
    let _armed = Armed::new(limit, 0);
    f()
}

/// Runs `f` with this thread's allocations after the first `left`
/// refused; also tells how many were refused.
fn armed_after<T>(left: usize, f: impl FnOnce() -> T) -> (T, usize) {
    ARM.with(|arm| arm.refused.set(0));
    let out = {
        let _armed = Armed::new(0, left);
        f()
    };
    (out, ARM.with(|arm| arm.refused.get()))
}

/// [`Memory`] with nothing refused while it works: the refusals are for
/// the writer's own allocations, not for the test's storage.
#[derive(Default)]
struct Spared(Memory);

impl Spared {
    fn spare<T>(f: impl FnOnce() -> T) -> T {
        let _spared = Armed::new(usize::MAX, 0);
        f()
    }
}

impl Storage for Spared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        Self::spare(|| self.0.read_at(path, offset, buf))
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        Self::spare(|| self.0.size(path))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        Self::spare(|| self.0.write(path, data))
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        Self::spare(|| self.0.append(path, data))
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Self::spare(|| self.0.list(dir, visit))
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        Self::spare(|| self.0.remove(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        Self::spare(|| self.0.rename(from, to))
    }
}

/// A database `db` that is already there, and its bytes.
fn existing() -> (Spared, Vec<u8>) {
    let storage = Spared::default();
    common::write_table(&storage.0, &[(1, vec![Value::Int(1), text("old")])]);
    let old = storage.0.get("db").expect("written");
    (storage, old)
}

/// `storage` holds `old` as `db` and nothing else.
fn left_as_it_was(storage: &Spared, old: &[u8]) {
    assert_eq!(storage.0.get("db").as_deref(), Some(old));
    assert_eq!(storage.0.paths(), ["db"]);
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
    let (storage, old) = existing();
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
    // The database there is left as it was, and no temporary pages.
    left_as_it_was(&storage, &old);
}

#[test]
fn a_fresh_index_without_room_fails_and_finish_tells_it() {
    let (storage, old) = existing();
    let mut writer = Writer::new(&storage, "db");
    let entries = [encode_record(&[Value::Int(1), Value::Int(1)])];
    assert_eq!(
        armed(1024, || writer.index(entries)),
        Err(Error::OutOfMemory)
    );
    // Memory is back, but the database would miss its index.
    let schema = [index_entry("i", "t", &["a"], 2)];
    assert_eq!(writer.finish(&schema, 0), Err(Error::OutOfMemory));
    left_as_it_was(&storage, &old);
}

#[test]
fn every_allocation_of_finish_refused_keeps_the_file_there() {
    let schema = [
        table_entry("t", &["a", "b"], 2),
        index_entry("i", "t", &["a"], 3),
    ];
    // The pages built with room; then each allocation of `finish` refused
    // in turn, the last (the copy buffer) included.
    for left in 0.. {
        assert!(left < 1000, "finish never ends");
        let (storage, old) = existing();
        let mut writer = Writer::new(&storage, "db");
        let root = writer.table([(1, encode_record(&[Value::Int(2), text("new")]))]);
        assert_eq!(root, Ok(2));
        let index = writer.index([encode_record(&[Value::Int(2), Value::Int(1)])]);
        assert_eq!(index, Ok(3));
        let (finished, refused) = armed_after(left, || writer.finish(&schema, 0));
        if refused == 0 {
            assert!(left > 3, "finish allocates names, page 1 and its buffer");
            assert_eq!(finished, Ok(()));
            assert_eq!(storage.0.paths(), ["db"]);
            break;
        }
        assert_eq!(finished, Err(Error::OutOfMemory), "allocation {left}");
        left_as_it_was(&storage, &old);
    }
}

/// A table's rows: id and record.
type Rows = Vec<(i64, Vec<u8>)>;

/// Rows over several leaves and a level above, the last one with
/// overflow pages; index entries over several leaves and a level above.
fn sample() -> (Rows, Vec<Vec<u8>>) {
    let mut rows: Rows = (1..=1200)
        .map(|i| {
            (
                i,
                encode_record(&[Value::Int(i), text(&format!("row {i:06}"))]),
            )
        })
        .collect();
    rows.push((
        1201,
        encode_record(&[Value::Int(1201), Value::Blob(vec![9; 10_000])]),
    ));
    let entries = (1..=1201)
        .map(|i| encode_record(&[text(&format!("key {i:06}")), Value::Int(i)]))
        .collect();
    (rows, entries)
}

#[test]
fn every_allocation_refused_while_writing_is_an_error_and_keeps_the_file_there() {
    let (rows, entries) = sample();
    let write = |storage: &Spared, schema: &[_]| {
        let mut writer = Writer::new(storage, "db");
        let table = writer.table(rows.clone());
        let index = writer.index(entries.clone());
        (table, index, writer.finish(schema, 1))
    };
    // Where the roots go, from a write with room.
    let (storage, _) = existing();
    let (table, index, _) = write(&storage, &[]);
    let (table, index) = (table.expect("table"), index.expect("index"));
    let schema = [
        table_entry("t", &["a", "b"], table),
        index_entry("i", "t", &["b"], index),
    ];
    // The n-th allocation refused, for every n until the write needs no
    // more.
    for left in 0.. {
        assert!(left < 100_000, "the write never ends");
        let (storage, old) = existing();
        let (rows_now, entries_now) = (rows.clone(), entries.clone());
        let ((got_table, got_index, finished), refused) = armed_after(left, || {
            let mut writer = Writer::new(&storage, "db");
            let got_table = writer.table(rows_now);
            let got_index = writer.index(entries_now);
            (got_table, got_index, writer.finish(&schema, 1))
        });
        if refused == 0 {
            assert!(left > 1000, "a write of many pages ({left} allocations)");
            assert_eq!(
                (got_table, got_index, finished),
                (Ok(table), Ok(index), Ok(()))
            );
            let db = Database::open(&storage, "db").expect("opens");
            assert_eq!(common::read_rows(&db, table).expect("rows"), rows);
            assert_eq!(storage.0.paths(), ["db"]);
            break;
        }
        assert_eq!(refused, 1, "stops at the first refusal (allocation {left})");
        assert!(matches!(got_table, Ok(t) if t == table) || got_table == Err(Error::OutOfMemory));
        assert!(matches!(got_index, Ok(i) if i == index) || got_index == Err(Error::OutOfMemory));
        assert_eq!(finished, Err(Error::OutOfMemory), "allocation {left}");
        left_as_it_was(&storage, &old);
    }
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
