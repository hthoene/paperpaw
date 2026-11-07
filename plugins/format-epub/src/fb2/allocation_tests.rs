//! Refuse ZIP buffers before unpacking can write the conversion cache.
extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;
use paperpaw_zip::testing::ZipBuilder;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

std::thread_local! {
    // Refuse a selected allocation size after skipping N matches.
    static REFUSE: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}
struct Allocator;
fn refused(size: usize) -> bool {
    REFUSE
        .try_with(|slot| match slot.get() {
            Some((wanted, skip)) if wanted == 0 || wanted == size => {
                if skip == 0 {
                    slot.set(None);
                    true
                } else {
                    slot.set(Some((wanted, skip - 1)));
                    false
                }
            }
            _ => false,
        })
        .unwrap_or(false)
}
// SAFETY: successful allocations and all frees are delegated unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refused(layout.size()) {
            core::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refused(size) {
            core::ptr::null_mut()
        } else {
            unsafe { System.realloc(ptr, layout, size) }
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn archive_open_and_entry_allocation_refusals_stay_out_of_memory() {
    // ZIP opening reads the 46-byte central record once; enumeration rereads it.
    for failure in [(0, 0), (46, 1)] {
        let storage = MemoryStorage::new().with(
            "book.zip",
            ZipBuilder::new()
                .stored("book.FB2", "<FictionBook/>")
                .build(),
        );
        REFUSE.set(Some(failure));
        let result = unpack(&storage, "book.zip", "output");
        let pending = REFUSE.replace(None);
        assert!(pending.is_none(), "selected allocation was not reached");
        assert!(matches!(result, Err(DocumentError::OutOfMemory)));
        assert!(!storage.exists("output"));
    }
}
