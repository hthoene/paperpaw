//! Test-only allocator that refuses the n-th allocation, to prove a code path
//! reports `Error::NoMemory` (and recovers) where the standard collections
//! would abort. Include with `#[path]`, then install as the test binary's
//! global allocator:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: fail_alloc::Failing = fail_alloc::Failing;
//! ```
use paperpaw_sdk::native::Error;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

std::thread_local! {
    /// The allocation to refuse, counted from 1; 0 turns counting off.
    static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

pub struct Failing;

fn refuse() -> bool {
    let at = REFUSE_AT.with(Cell::get);
    if at == 0 {
        return false;
    }
    let n = COUNT.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    n == at
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

/// Runs `build` with nothing refused or counted: the test's own allocations
/// (its fixtures, a server double) are not the code's.
#[allow(dead_code)]
pub fn exempt<T>(build: impl FnOnce() -> T) -> T {
    let at = REFUSE_AT.with(|a| a.replace(0));
    let value = build();
    REFUSE_AT.with(|a| a.set(at));
    value
}

/// Runs `run` once to count its allocations, then once more with each of them
/// refused in turn. A refused run must end in `Err(Error::NoMemory)` or succeed
/// anyway; an abort (an infallible allocation) fails the whole test binary.
/// Returns how many allocations one run makes.
pub fn sweep<T>(mut run: impl FnMut() -> Result<T, Error>) -> usize {
    REFUSE_AT.with(|a| a.set(usize::MAX));
    COUNT.with(|c| c.set(0));
    let first = run();
    let total = COUNT.with(Cell::get);
    REFUSE_AT.with(|a| a.set(0));
    assert!(first.is_ok(), "the unrefused run must succeed");
    drop(first);
    for n in 1..=total {
        COUNT.with(|c| c.set(0));
        REFUSE_AT.with(|a| a.set(n));
        let result = run();
        REFUSE_AT.with(|a| a.set(0));
        if let Err(error) = &result {
            assert_eq!(*error, Error::NoMemory, "allocation {n} of {total} refused");
        }
    }
    // Recovery: the same run succeeds afterwards.
    assert!(run().is_ok(), "the run recovers after refusals");
    total
}
