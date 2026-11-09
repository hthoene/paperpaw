//! `Params::try_set` and `Params::try_clone` under a real allocator: a refused allocation is an
//! error, the parameters stay as they were, nothing aborts.
#![allow(clippy::unwrap_used, reason = "test setup and checks fail loudly")]

use paperpaw_sdk::Params;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static REFUSE: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static LAST_SIZE: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards to `System`, refusing (null) while asked to.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let refuse = REFUSE.try_with(Cell::get).unwrap_or(false);
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        let _ = LAST_SIZE.try_with(|n| n.set(layout.size()));
        if refuse {
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
static ALLOCATOR: Counting = Counting;

fn allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCATIONS.with(Cell::get);
    let value = operation();
    (value, ALLOCATIONS.with(Cell::get) - before)
}

fn refused<T>(operation: impl FnOnce() -> T) -> T {
    REFUSE.with(|refuse| refuse.set(true));
    let value = operation();
    REFUSE.with(|refuse| refuse.set(false));
    value
}

#[test]
fn try_set_with_a_kept_key_allocates_the_value_and_the_row() {
    let mut params = Params::new();
    let (result, count) = allocations(|| params.try_set("path", "a.txt"));
    result.unwrap();
    assert_eq!(count, 2, "the row, then the value's copy");
    assert_eq!(params.get("path"), Some("a.txt"));
}

#[test]
fn try_set_gives_an_error_where_the_heap_refuses_and_changes_nothing() {
    let mut params = Params::new().with("path", "a.txt");
    // A new key: nothing added.
    assert!(refused(|| params.try_set("other", "x")).is_err());
    assert_eq!(params.get("other"), None);
    assert_eq!(params.iter().count(), 1);
    // A replaced key: the old value stays.
    assert!(refused(|| params.try_set("path", "b.txt")).is_err());
    assert_eq!(params.get("path"), Some("a.txt"));
    // Heap back: it works as before.
    params.try_set("path", "b.txt").unwrap();
    assert_eq!(params.get("path"), Some("b.txt"));
}

#[test]
fn an_empty_value_needs_no_copy() {
    let mut params = Params::new().with("a", "x");
    let (result, count) = allocations(|| refused(|| params.try_set("a", "")));
    result.unwrap();
    assert_eq!(count, 0);
    assert_eq!(params.get("a"), Some(""));
}

#[test]
fn try_clone_copies_and_gives_an_error_where_the_heap_refuses() {
    let params = Params::new()
        .with("path", "a.txt")
        .with(String::from("owned"), "b");
    let copy = params.try_clone().unwrap();
    assert_eq!(copy, params);
    drop(params.clone());
    assert!(refused(|| params.try_clone()).is_err());
    assert_eq!(params.get("path"), Some("a.txt"));
    // Empty: no allocation to refuse.
    assert!(refused(|| Params::new().try_clone()).is_ok());
}
