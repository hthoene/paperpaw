//! Fail only new wrapper allocations; legacy transform allocation is a separate activation gate.
#![cfg(feature = "reader-filter")]
#![allow(clippy::expect_used, reason = "test fixture construction")]
use paperpaw_plugin_aids::{
    pictures,
    reader_filter::{self, Error},
};
use paperpaw_sdk::document::{Block, BlockKind};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static FAIL_AT: Cell<usize> = const { Cell::new(usize::MAX) }; }
struct Allocator;
fn refuse() -> bool {
    FAIL_AT
        .try_with(|count| {
            let n = count.get();
            if n == usize::MAX {
                return false;
            }
            count.set(if n == 0 { usize::MAX } else { n - 1 });
            n == 0
        })
        .unwrap_or(false)
}
// SAFETY: all successful operations and deallocations delegate unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            std::ptr::null_mut()
        } else {
            unsafe { System.realloc(ptr, layout, size) }
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            System.dealloc(ptr, layout);
        }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn every_encoder_reservation_refuses_without_writing() {
    for n in 0..3 {
        let mut output = vec![0x55; 3840];
        FAIL_AT.set(n);
        let result = reader_filter::encode(true, "a=b;c=d", pictures::SHOW, "Picture", &mut output);
        FAIL_AT.set(usize::MAX);
        assert_eq!(result, Err(Error::NoMemory), "reservation {n}");
        assert!(output.iter().all(|b| *b == 0x55));
    }
}

#[test]
fn every_application_copy_refuses_before_block_mutation() {
    let mut bytes = vec![0; 3840];
    let len = reader_filter::encode(true, "a=b;c=d", pictures::CAPTION, "Picture", &mut bytes)
        .expect("fixture");
    // Pair vector plus two nonempty strings per pair. These all precede transforms.
    for n in 0..5 {
        let original = Block::new(BlockKind::Paragraph, 0, 3, "a c".to_owned());
        let mut block = original.clone();
        FAIL_AT.set(n);
        let result = reader_filter::apply(&bytes[..len], "book.epub", &mut block);
        FAIL_AT.set(usize::MAX);
        assert_eq!(result, Err(Error::NoMemory), "reservation {n}");
        assert_eq!(block, original);
    }
}
