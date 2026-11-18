//! Test-only allocator that refuses the n-th allocation the plugin makes, to
//! prove its callbacks answer `Error::NoMemory` where the standard collections
//! would abort the process. Allocations made inside the test host's own
//! callbacks (`native_host::HostScope`) are not counted or refused: they are
//! the host's. Include after `native_host.rs` and install:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: fail_alloc::Failing = fail_alloc::Failing;
//! ```
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

std::thread_local! {
    /// The allocation to refuse, counted from 1; 0 turns counting off.
    static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

pub struct Failing;

fn refuse() -> bool {
    let at = REFUSE_AT.try_with(Cell::get).unwrap_or(0);
    if at == 0 || super::host::HOST_DEPTH.try_with(Cell::get).unwrap_or(1) != 0 {
        return false;
    }
    COUNT
        .try_with(|count| {
            count.set(count.get() + 1);
            count.get() == at
        })
        .unwrap_or(false)
}

// SAFETY: every call forwards to `System` unchanged, or returns null to refuse.
unsafe impl GlobalAlloc for Failing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout contract passes through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            return std::ptr::null_mut();
        }
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

/// Runs `run` with the `at`-th plugin allocation refused (`usize::MAX`: none
/// refused, only counted); returns its result and how many allocations the
/// plugin made.
pub fn armed<T>(at: usize, run: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|count| count.set(0));
    REFUSE_AT.with(|refuse| refuse.set(at));
    let result = run();
    REFUSE_AT.with(|refuse| refuse.set(0));
    (result, COUNT.with(Cell::get))
}
