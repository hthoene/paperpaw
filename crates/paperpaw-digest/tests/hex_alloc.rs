//! `hex::decode_into` and `hex::decode_array` allocate nothing, and
//! `hex::try_decode` reports a refused reservation instead of aborting.

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_digest::hex;

std::thread_local! {
    /// Allocations seen and whether to refuse them, for this thread.
    static SEEN: Cell<usize> = const { Cell::new(0) };
    static REFUSE: Cell<bool> = const { Cell::new(false) };
}

struct Counting;

// SAFETY: passes every allocation on to the system's allocator, or refuses it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        SEEN.try_with(|seen| seen.set(seen.get() + 1)).ok();
        if REFUSE.try_with(Cell::get).unwrap_or(false) {
            return core::ptr::null_mut();
        }
        // SAFETY: as the caller asked.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations `run` makes, with every one refused when `refuse`.
fn allocations<T>(refuse: bool, run: impl FnOnce() -> T) -> (usize, T) {
    SEEN.with(|seen| seen.set(0));
    REFUSE.with(|flag| flag.set(refuse));
    let out = run();
    REFUSE.with(|flag| flag.set(false));
    (SEEN.with(Cell::get), out)
}

#[test]
fn decoding_into_a_buffer_or_an_array_allocates_nothing() {
    let text = "00ff7F80".repeat(8);
    let mut out = [0; 32];
    let (count, got) = allocations(true, || hex::decode_into(&text, &mut out).map(<[u8]>::len));
    assert_eq!((count, got), (0, Some(32)));
    let (count, got) = allocations(true, || hex::decode_array::<32>(&text).is_some());
    assert_eq!((count, got), (0, true));
    let (count, got) = allocations(true, || hex::decode_array::<32>("zz").is_some());
    assert_eq!((count, got), (0, false));
}

#[test]
fn a_refused_reservation_is_an_error_not_an_abort() {
    let (count, result) = allocations(true, || hex::try_decode("00ff"));
    assert!(result.is_err());
    assert_eq!(count, 1, "the one reservation was asked for and refused");
    let (_, result) = allocations(false, || hex::try_decode("00ff"));
    assert_eq!(result.unwrap(), Some(vec![0, 0xff]));
    let (count, result) = allocations(true, || hex::try_decode("0"));
    assert_eq!(
        (count, result.unwrap()),
        (0, None),
        "odd text reserves nothing"
    );
}
