//! Memory a hostile position asks for: the largest single allocation is
//! recorded by this test binary's allocator (the only test in it, so no
//! other test's allocations count).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::book;
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, Locator};

/// The system allocator, noting the largest allocation since reset.
struct Largest;

static LARGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Largest {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LARGEST.fetch_max(new_size, Ordering::Relaxed);
        // SAFETY: as the caller asked of this allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Largest = Largest;

#[test]
fn xpointers_of_positions_far_past_a_chapters_end_ask_little_memory() {
    // A position kept from a longer edition of the book, or a stray one:
    // `xpointer` reads from the last text node to the position into one
    // buffer, however far that is. 64 MB here; up to 4 GB with an offset
    // near `u32::MAX`, which aborts a device.
    let chapter: String = (0..50).map(|i| format!("<p>Paragraph {i}.</p>")).collect();
    let storage = MemoryStorage::new().with("book.epub", book(&[chapter], &[]));
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    let end = doc.section_len(0);
    LARGEST.store(0, Ordering::Relaxed);
    let xpointer = doc.xpointer(&storage, Locator::new(0, end + (64 << 20)));
    let largest = LARGEST.load(Ordering::Relaxed);
    assert!(xpointer.is_some());
    assert!(largest < 1 << 20, "{largest} bytes in one allocation");
}
