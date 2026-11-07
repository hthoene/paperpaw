//! Refuse every allocation in the new encoder and atomic application.
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
    for n in 0..128 {
        let original = Block::new(BlockKind::Image, 0, 3, "a c".to_owned());
        let mut block = original.clone();
        FAIL_AT.set(n);
        let result = reader_filter::apply(&bytes[..len], "book.epub", &mut block);
        FAIL_AT.set(usize::MAX);
        if result.is_ok() {
            assert!(n > 0);
            return;
        }
        assert_eq!(result, Err(Error::NoMemory), "reservation {n}");
        assert_eq!(block, original);
    }
    panic!("allocation sweep did not reach a successful application");
}

#[test]
fn paragraph_bold_growth_refuses_without_changing_the_block() {
    use paperpaw_sdk::document::{Run, TextStyle};
    let mut bytes = vec![0; 3840];
    let len =
        reader_filter::encode(true, "", pictures::SHOW, "Picture", &mut bytes).expect("fixture");
    let text = "alpha beta gamma delta epsilon zeta eta theta";
    let mut original = Block::new(BlockKind::Paragraph, 4, 100, text.to_owned());
    original.runs.push(Run::new(6, 10, TextStyle::ITALIC));
    original.map.checkpoint(0, 8);
    original.map.checkpoint(11, 30);
    let mut expected = original.clone();
    reader_filter::apply(&bytes[..len], "book.epub", &mut expected).expect("baseline");
    assert!(expected.runs.len() > original.runs.len());
    assert!(expected.runs.contains(&Run::new(0, 3, TextStyle::BOLD)));
    assert!(expected.runs.contains(&Run::new(6, 10, TextStyle::ITALIC)));
    for n in 0..128 {
        let mut block = original.clone();
        FAIL_AT.set(n);
        let result = reader_filter::apply(&bytes[..len], "book.epub", &mut block);
        FAIL_AT.set(usize::MAX);
        if result.is_ok() {
            assert!(
                n > 3,
                "must exercise run growth after the initial edit copy"
            );
            assert_eq!(block, expected);
            return;
        }
        assert_eq!(result, Err(Error::NoMemory), "reservation {n}");
        assert_eq!(block, original, "reservation {n} changed the original");
    }
    panic!("allocation sweep did not reach a successful bold transformation");
}
