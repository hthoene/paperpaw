//! The most heap the provider holds while it takes the end of a request
//! with the largest body it reads (`MAX_BODY`, all of it the summary): the
//! answer kept for the last word is gone by then, and the body goes as soon
//! as the summary is out of it. Measured with the scripted host's own
//! allocations (the event, the body's parts) in the count.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
#[path = "support/provider.rs"]
mod support;
use paperpaw_plugin_wikipedia::{NATIVE_ENTRY, native_model::MAX_BODY};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use support::{PENDING, ask, done_with, host_with};

/// Counts the bytes allocated now, and the most since the peak was reset.
struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: passes every call on to the system's allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as the caller asked.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most the heap is above `base` while `run` runs.
fn high_water(base: usize, run: impl FnOnce()) -> usize {
    PEAK.store(NOW.load(Ordering::Relaxed), Ordering::Relaxed);
    run();
    PEAK.load(Ordering::Relaxed).saturating_sub(base)
}

/// At most this much heap above the loaded plugin for the end of a request:
/// the largest body (16 KiB), the summary taken out of it (as long), the
/// `lookup_ready` scratch and the reply buffer of a part, with the scripted
/// host's share. The answer kept for another word is not among them.
const BOUND: usize = 40 * 1024;

#[test]
fn the_end_of_a_request_with_the_largest_body_stays_in_bound() {
    let head = r#"{"title":"Katze","extract":""#;
    let tail = r#""}"#;
    let body: &'static str = Box::leak(
        format!(
            "{head}{}{tail}",
            "a".repeat(MAX_BODY - head.len() - tail.len())
        )
        .into_boxed_str(),
    );
    assert_eq!(body.len(), MAX_BODY);
    let (host, _log) = host_with(200, body);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let base = NOW.load(Ordering::Relaxed);
    // A summary as large is kept for another word first.
    ask(&p, "Hund", "de");
    done_with(&p, 1, 200, 0, body.len());
    assert_eq!(ask(&p, "Hund", "de").2 & PENDING, 0, "kept");
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    let grown = high_water(base, || {
        done_with(&p, 2, 200, 0, body.len());
    });
    eprintln!("PEAK_DONE_BYTES {grown}");
    assert!(grown <= BOUND, "{grown} bytes over {BOUND}");
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, 0, "kept");
}
