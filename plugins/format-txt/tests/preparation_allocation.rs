//! Every preparation allocation is fallible before cache writes.
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
fn preparation_refuses_each_scratch_allocation() {
    use paperpaw_plugin_format_txt::preparation::Preparation;
    use paperpaw_sdk::native::Error;
    for n in 0..2 {
        FAIL_AT.set(n);
        let result = Preparation::new(1024, false);
        FAIL_AT.set(usize::MAX);
        assert!(matches!(result, Err(Error::NoMemory)), "reservation {n}");
    }
    assert!(Preparation::new(1024, false).is_ok());
}
