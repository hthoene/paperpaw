//! A memory shortage while the journal's record is made is
//! `StorageError::OutOfMemory`, never `TooLarge` (which stays the size
//! of a thing that will never fit).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;
use super::*;
use core::cell::Cell;
use std::alloc::{GlobalAlloc, Layout, System};
use std::string::String;

std::thread_local! {
    /// The size of the one allocation this thread refuses (0: none).
    static REFUSE: Cell<usize> = const { Cell::new(0) };
}

struct Refusing;

// SAFETY: passes every call on to the system allocator, or refuses with
// null, which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if REFUSE.try_with(Cell::get).unwrap_or(0) == layout.size() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: allocated by `System` with this layout.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

#[test]
fn a_move_record_the_heap_cannot_hold_is_out_of_memory() {
    // Paths of lengths no other allocation here has.
    let (from, to) = (String::from("a").repeat(301), String::from("b").repeat(307));
    let size = MOVE_HEAD + from.len() + 1 + to.len();
    REFUSE.with(|refuse| refuse.set(size));
    let refused = move_record(&from, &to);
    REFUSE.with(|refuse| refuse.set(0));
    assert_eq!(refused, Err(StorageError::OutOfMemory));
    assert_eq!(move_record(&from, &to).unwrap().len(), size);
}
