//! Owner-side widget/status adaptation must allocate no heap storage.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_sdk::native::{
    callbacks::FrameRect,
    stage_schemas::{core_status_v1 as status, core_widget_v1 as widget},
    ui::{
        WidgetTarget,
        stages::{StatusAnswer, StatusCall, WidgetCall},
    },
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
fn full_widget_and_status_replies_allocate_nothing() {
    let mut input = [0; 1024];
    let mut out = [0; 3840];
    let n = widget::input_encode(
        &[widget::Record::WidgetTargets(widget::WidgetTargets::new(
            1, 0, 0, 300, 400,
        ))],
        &mut input,
    )
    .unwrap();
    let targets = [WidgetTarget::new(1, FrameRect::new(0, 0, 10, 10)); widget::REPLY_ROWS];
    let n = no_alloc(|| {
        WidgetCall::decode(&input[..n])
            .unwrap()
            .reply_targets(&targets, &mut out)
            .unwrap()
    });
    assert_eq!(
        widget::reply_decode(&out[..n]).unwrap().len(),
        widget::REPLY_ROWS
    );
    let items =
        "fixture.a\nfixture.b\nfixture.c\nfixture.d\nfixture.e\nfixture.f\nfixture.g\nfixture.h";
    let values = [
        "fixture.a",
        "fixture.b",
        "fixture.c",
        "fixture.d",
        "fixture.e",
        "fixture.f",
        "fixture.g",
        "fixture.h",
    ]
    .map(|item| StatusAnswer::text(item, "text").unwrap());
    // Two bare service.ui_param records (tag 1057): title=Book, chapter=Part1.
    // Keep a golden wire fixture here so this integration test needs no private codec.
    const CONTEXT: &[u8] = b"\xa1\x08\x0c\0\x05title\x04Book\xa1\x08\x0f\0\x07chapter\x05Part1";
    let n = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            items, CONTEXT,
        ))],
        &mut input,
    )
    .unwrap();
    let n = no_alloc(|| {
        let call = StatusCall::decode(&input[..n]).unwrap();
        let mut context = call.context();
        let title = context.next().unwrap().unwrap();
        let chapter = context.next().unwrap().unwrap();
        assert_eq!((title.key(), title.value()), ("title", "Book"));
        assert_eq!((chapter.key(), chapter.value()), ("chapter", "Part1"));
        assert!(input.as_ptr_range().contains(&title.value().as_ptr()));
        assert!(input.as_ptr_range().contains(&chapter.value().as_ptr()));
        assert!(context.next().is_none());
        call.reply(&values, &mut out).unwrap()
    });
    assert_eq!(status::reply_decode(&out[..n]).unwrap().len(), values.len());
}
