//! Count only allocations on this test's thread, after catalog setup.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use crate::{Catalog, Translator};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

std::thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every allocation and deallocation forwards unchanged to System.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        // SAFETY: the caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies a live allocation and its layout.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn borrowed_plural_never_allocates() {
    let mut translator = Translator::new();
    translator.add(Catalog::parse("[meta]\nlanguage = en\nplural = one-other\n[messages]\nbooks.one = One\nbooks.other = Other\n").unwrap());
    let before = ALLOCATIONS.with(Cell::get);
    for count in [0, 1, 2, 5] {
        assert!(translator.get_plural("books", count).is_some());
        assert_eq!(translator.get_plural("missing", count), None);
    }
    assert_eq!(ALLOCATIONS.with(Cell::get), before);
}
