use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

pub struct Counting;

thread_local! {
    static USAGE: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

fn change(removed: usize, added: usize) {
    let _ = USAGE.try_with(|usage| {
        if let Some((live, peak)) = usage.get() {
            let live = live - removed + added;
            usage.set(Some((live, peak.max(live))));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            change(0, layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        change(layout.size(), 0);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            change(layout.size(), size);
        }
        pointer
    }
}

pub fn measure(operation: impl FnOnce()) -> (usize, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            USAGE.with(|usage| usage.set(None));
        }
    }
    USAGE.with(|usage| {
        assert!(usage.get().is_none());
        usage.set(Some((0, 0)));
    });
    let reset = Reset;
    operation();
    let usage = USAGE.with(|usage| usage.get().unwrap());
    drop(reset);
    usage
}
