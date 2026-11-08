//! `Id::try_clone` and `Id::try_parse` under a real allocator: kept ids never allocate, parsed
//! ids take one allocation of their length and give `None` where it is
//! refused, never aborting.
#![allow(clippy::unwrap_used, reason = "test setup and checks fail loudly")]

use paperpaw_sdk::{Id, IdError};
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
fn a_kept_id_copies_without_allocating_even_on_a_full_heap() {
    const OPEN: Id = Id::new_static("battery.open");
    let (copy, count) = allocations(|| refused(|| OPEN.try_clone()));
    assert_eq!(count, 0);
    assert_eq!(copy.unwrap(), OPEN);
}

#[test]
fn a_parsed_id_takes_one_allocation_of_its_length() {
    let id = Id::parse("battery.open").unwrap();
    let (copy, count) = allocations(|| id.try_clone());
    let copy = copy.unwrap();
    assert_eq!(count, 1);
    assert_eq!(LAST_SIZE.with(Cell::get), "battery.open".len());
    assert_eq!(copy, id);
    // An independent copy: it outlives the original.
    drop(id);
    assert_eq!(copy.as_str(), "battery.open");
}

#[test]
fn a_parsed_id_gives_none_where_the_heap_refuses() {
    let id = Id::parse("battery.open").unwrap();
    assert!(refused(|| id.try_clone()).is_none());
    // Nothing changed: the original stays usable and copies again.
    assert_eq!(id.as_str(), "battery.open");
    assert_eq!(id.try_clone().unwrap(), id);
}

#[test]
fn a_placed_parsed_id_copies_as_clone_does() {
    let id = Id::parse("widgets.clock@2.face").unwrap();
    let copy = id.try_clone().unwrap();
    assert_eq!(copy, id.clone());
    assert_eq!(copy.unplaced(), id.unplaced());
}

#[test]
fn try_parse_checks_first_then_takes_one_allocation_of_its_length() {
    let (id, count) = allocations(|| Id::try_parse("battery.open"));
    assert_eq!(count, 1);
    assert_eq!(LAST_SIZE.with(Cell::get), "battery.open".len());
    assert_eq!(id.unwrap(), Id::new_static("battery.open"));
    for invalid in ["", "Battery.open", "a..b", "a@1@2"] {
        let (result, count) = allocations(|| refused(|| Id::try_parse(invalid)));
        assert_eq!((result, count), (Err(IdError::Invalid), 0), "{invalid}");
    }
}

#[test]
fn try_parse_gives_no_memory_where_the_heap_refuses() {
    assert_eq!(
        refused(|| Id::try_parse("battery.open")),
        Err(IdError::NoMemory)
    );
    let id = Id::try_parse("widgets.clock@2.face").unwrap();
    assert_eq!(id, Id::parse("widgets.clock@2.face").unwrap());
    assert_eq!(id.unplaced(), Some(Id::new_static("widgets.clock.face")));
}
