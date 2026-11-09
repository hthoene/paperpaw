//! Format provider scalar replies borrow their model without allocating.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_sdk::{
    document::Metadata,
    native::{format::FormatCall, stage_schemas::core_format_v1 as schema},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
struct Tracking;
thread_local! { static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) }; }
fn track() {
    let _ = ALLOCS.try_with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        track();
        // SAFETY: forwarding the allocator contract to the system allocator.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation came from this same system allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        track();
        // SAFETY: forwarding the original allocation and requested size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Tracking = Tracking;
fn no_alloc<T>(call: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOCS.with(|n| n.set(None));
        }
    }
    ALLOCS.with(|n| n.set(Some(0)));
    let reset = Reset;
    let value = call();
    let count = ALLOCS.with(Cell::get).unwrap();
    drop(reset);
    assert_eq!(count, 0);
    value
}

#[test]
fn metadata_and_text_decode_encode_allocate_nothing() {
    let metadata = Metadata::default()
        .title("界".repeat(1000))
        .authors(vec!["Author".into()]);
    let mut input = [0; 1024];
    let mut out = [0; 2048];
    for op in [5, 6] {
        let n = schema::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 0, 0, 0, 99, "", &[])
            .encode(&mut input)
            .unwrap();
        let n = no_alloc(|| {
            let call = FormatCall::decode(&input[..n]).unwrap();
            if op == 5 {
                call.reply_metadata(&metadata, 1, &mut out).unwrap()
            } else {
                call.reply_metadata_text(&metadata, &mut out).unwrap()
            }
        });
        assert_eq!(schema::reply_decode(&out[..n]).unwrap().len(), 1);
    }
}
