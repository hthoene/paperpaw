//! The fallible forms of `paperpaw_sdk::web` under a real allocator: refusing
//! each allocation in turn gives `Err`, never a panic, an abort or a partial
//! name; unrefused they give what the infallible forms give.
#![allow(clippy::unwrap_used, reason = "test setup and checks fail loudly")]

use paperpaw_sdk::web::{
    base_url, clean_name, file_name, percent_decode, try_base_url, try_clean_name, try_file_name,
    try_percent_decode,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Refusing;

thread_local! {
    /// Allocations before the one refused (`None`: none is).
    static UNTIL_REFUSED: Cell<Option<usize>> = const { Cell::new(None) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn refuse() -> bool {
    let _ = COUNT.try_with(|count| count.set(count.get() + 1));
    UNTIL_REFUSED
        .try_with(|until| match until.get() {
            Some(1) => {
                until.set(None);
                true
            }
            Some(n) => {
                until.set(Some(n - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}

// SAFETY: forwards to `System`, refusing (null) the chosen allocation.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            core::ptr::null_mut()
        } else {
            // SAFETY: as `GlobalAlloc::alloc`.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: as `GlobalAlloc::dealloc`.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            core::ptr::null_mut()
        } else {
            // SAFETY: as `GlobalAlloc::realloc`.
            unsafe { System.realloc(pointer, layout, size) }
        }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Runs `operation` once to count its allocations, then once more with each
/// of them refused in turn: every refused run must be an `Err`. Returns how
/// many allocations one run makes.
fn sweep<T: PartialEq + core::fmt::Debug, E>(mut operation: impl FnMut() -> Result<T, E>) -> usize {
    COUNT.with(|count| count.set(0));
    let whole = operation().ok().unwrap();
    let total = COUNT.with(Cell::get);
    for n in 1..=total {
        UNTIL_REFUSED.with(|until| until.set(Some(n)));
        let result = operation();
        UNTIL_REFUSED.with(|until| until.set(None));
        assert!(
            result.is_err(),
            "allocation {n} of {total} was refused but the run went on"
        );
    }
    // The same run succeeds afterwards, with the same result.
    assert_eq!(operation().ok().unwrap(), whole);
    total
}

const NAMES: &[&str] = &[
    "Chapter 1.",
    " Kafka: Die  Verwandlung? ",
    "con.txt",
    "a\u{a0}b \u{1f4da}x",
    "https://a.org/books/Die%20Verwandlung.epub?x=1",
    "https://a.org/%ff%fe.epub",
    "https://a.org/",
    "nas.local:8080/x",
    "%e4%b8",
    "",
];

#[test]
fn a_refused_allocation_gives_an_error_and_no_name() {
    for name in NAMES {
        assert_eq!(sweep(|| try_clean_name(name, 100)), 1, "{name:?}");
        sweep(|| try_percent_decode(name));
        sweep(|| try_file_name(name));
        assert!(sweep(|| try_base_url(name)) <= 1, "{name:?}");
    }
}

#[test]
fn unrefused_the_forms_agree() {
    for name in NAMES {
        assert_eq!(try_clean_name(name, 100).unwrap(), clean_name(name, 100));
        assert_eq!(try_percent_decode(name).unwrap(), percent_decode(name));
        assert_eq!(try_file_name(name).unwrap(), file_name(name));
        assert_eq!(try_base_url(name).unwrap(), base_url(name));
    }
}
