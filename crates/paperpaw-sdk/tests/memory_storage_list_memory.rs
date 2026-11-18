//! `MemoryStorage::list` under a refusing allocator (todo 571): out-of-memory
//! tests of other code must not trip over the test storage, so a refused
//! allocation is `OutOfMemory` and listing a folder of files allocates nothing.
#![allow(clippy::unwrap_used, reason = "test setup and checks fail loudly")]

use paperpaw_platform::{Storage as _, StorageError};
use paperpaw_sdk::MemoryStorage;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Refusing;

thread_local! {
    static REFUSE: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards to `System`, refusing (null) while asked to.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        if REFUSE.try_with(Cell::get).unwrap_or(false) {
            core::ptr::null_mut()
        } else {
            // SAFETY: as `GlobalAlloc::alloc`.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: as `GlobalAlloc::dealloc`.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Runs `operation` with every allocation refused; gives its value and the
/// number of allocations asked for.
fn refused<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    /// Puts the heap back even if `operation` panics.
    struct Back;
    impl Drop for Back {
        fn drop(&mut self) {
            REFUSE.with(|refuse| refuse.set(false));
        }
    }
    let before = ALLOCATIONS.with(Cell::get);
    REFUSE.with(|refuse| refuse.set(true));
    let back = Back;
    let value = operation();
    drop(back);
    (value, ALLOCATIONS.with(Cell::get) - before)
}

fn storage() -> MemoryStorage {
    MemoryStorage::new()
        .with("books/a.txt", "aa")
        .with("books/b.epub", "b")
        .with("books/sub/c.txt", "c")
        .with("books/sub.d/e.txt", "e")
        .with("notes/n.txt", "n")
}

#[test]
fn listing_a_folder_of_files_allocates_nothing() {
    let storage = storage();
    let mut seen = 0_usize;
    let (result, count) = refused(|| storage.list("notes", &mut |_| seen += 1));
    assert_eq!(result, Ok(()));
    assert_eq!(seen, 1);
    assert_eq!(count, 0, "no allocation for a folder without subfolders");
}

#[test]
fn a_refused_allocation_for_subfolders_is_out_of_memory_and_visits_nothing() {
    let storage = storage();
    let mut seen = 0_usize;
    let (result, _) = refused(|| storage.list("books", &mut |_| seen += 1));
    assert_eq!(result, Err(StorageError::OutOfMemory));
    assert_eq!(seen, 0, "nothing visited before the refusal");
    let (result, _) = refused(|| storage.list("", &mut |_| seen += 1));
    assert_eq!(result, Err(StorageError::OutOfMemory));
    assert_eq!(seen, 0);
}

#[test]
fn with_memory_it_lists_files_then_subfolders_in_name_order() {
    let storage = storage();
    let mut names = Vec::new();
    storage
        .list("books", &mut |entry| {
            names.push((String::from(entry.name), entry.is_dir));
        })
        .unwrap();
    let want = [
        ("a.txt", false),
        ("b.epub", false),
        ("sub", true),
        ("sub.d", true),
    ];
    let want: Vec<_> = want.iter().map(|(n, d)| (String::from(*n), *d)).collect();
    assert_eq!(names, want);
    assert_eq!(
        storage.list("nowhere", &mut |_| {}),
        Err(StorageError::NotFound)
    );
}
