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

#[test]
fn unsupported_unrelated_entry_does_not_hide_a_readable_book() {
    use paperpaw_sdk::zip::{ZipArchive, ZipError};
    let mut bytes = ZipBuilder::new()
        .stored("junk.bin", "unrelated")
        .stored("book.fb2", "<FictionBook/>")
        .build();
    let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    // The first member uses a compression method the ZIP reader doesn't support.
    bytes[central + 10..central + 12].copy_from_slice(&99u16.to_le_bytes());
    let storage = MemoryStorage::new().with("book.zip", bytes);
    let archive = ZipArchive::open(&storage, "book.zip").unwrap();
    // Entries are in hash order, not archive order: prove the error precedes the book.
    assert!(matches!(
        archive.entries(&storage).next(),
        Some(Err(ZipError::Unsupported(_)))
    ));
    unpack(&storage, "book.zip", "output").unwrap();
    assert_eq!(
        paperpaw_sdk::read_file(&storage, "output", 100).unwrap(),
        b"<FictionBook/>"
    );
}
