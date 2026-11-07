//! The budget allocator's accounting, with the allocator itself (it is
//! the global allocator of this binary, so of its tests too): what is
//! charged and credited back, what is refused, which budget holds, and
//! how the numbers are shown.
//!
//! The counters and the budget are global; the tests that read them,
//! and the device's tests that run a kernel under a budget, take
//! [`LOCK`] so they do not see each other's allocations. Other tests
//! allocate untracked, which the counters leave alone.
//!
//! What a test charges it frees before it lets [`LOCK`] go. Nothing is
//! printed while tracking, a panic's message neither: the test harness
//! keeps a test's output in a buffer of its own, which would grow
//! charged and be freed later on another thread, in another test's
//! count.

use std::alloc::Layout;
use std::sync::{Mutex, MutexGuard};

use super::*;

/// Held by every test of this binary that tracks allocations or sets a
/// budget.
pub(crate) static LOCK: Mutex<()> = Mutex::new(());

/// The counters to themselves, the budget lifted, as no profile set one.
fn alone() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    OVERRIDDEN.store(false, Ordering::Relaxed);
    set_limit(None);
    guard
}

fn live() -> usize {
    stats().live
}

#[test]
fn tracked_allocations_are_charged_with_a_header_and_credited_back() {
    let _alone = alone();
    let before = live();
    let buffer = track(|| Vec::<u8>::with_capacity(1000));
    assert_eq!(live(), before + 1000 + HEADER);
    assert!(stats().peak >= before + 1000 + HEADER);
    // Freed outside the kernel's calls: still credited back.
    drop(buffer);
    assert_eq!(live(), before);
}

#[test]
fn untracked_memory_is_neither_charged_nor_credited() {
    let _alone = alone();
    let before = live();
    // The shell's own memory, freed by the kernel (e.g. a request's body
    // handed over): the count does not drop below what is charged.
    let shells = vec![0_u8; 4096];
    assert_eq!(live(), before);
    track(|| drop(shells));
    assert_eq!(live(), before);
}

#[test]
fn a_tracked_allocation_past_the_budget_is_refused_and_counted() {
    let _alone = alone();
    let before = stats();
    set_limit(Some(before.live + 2048));
    let refused = track(|| Vec::<u8>::new().try_reserve_exact(4096));
    let fits = track(|| Vec::<u8>::new().try_reserve_exact(512));
    let after = stats();
    set_limit(None);
    assert!(refused.is_err(), "4 KiB past a 2 KiB budget");
    assert!(fits.is_ok(), "512 bytes within it");
    assert_eq!(after.failed, before.failed + 1);
    // Nothing charged for what was refused, the small one freed again.
    assert_eq!(after.live, before.live);
    assert_eq!(after.limit, Some(before.live + 2048));
}

#[test]
fn the_last_refusal_is_remembered_until_cleared() {
    let _alone = alone();
    clear_last_refused();
    set_limit(Some(stats().live + 2048));
    let refused = track(|| Vec::<u8>::new().try_reserve_exact(4096));
    set_limit(None);
    assert!(refused.is_err());
    assert_eq!(last_refused(), 4096);
    clear_last_refused();
    assert_eq!(last_refused(), 0);
}

#[test]
fn untracked_allocations_are_never_refused() {
    let _alone = alone();
    set_limit(Some(live()));
    // The emulator's window and pictures are not the device's heap.
    let window = vec![0_u8; 64 * 1024];
    set_limit(None);
    assert_eq!(window.len(), 64 * 1024);
}

#[test]
fn a_budget_given_on_the_command_line_wins_over_the_profiles() {
    let _alone = alone();
    override_limit(Some(300 * 1024));
    set_limit(Some(256 * 1024));
    assert_eq!(stats().limit, Some(300 * 1024), "the profile's left aside");
    override_limit(None);
    set_limit(Some(256 * 1024));
    assert_eq!(stats().limit, None, "`none` lifts it for the whole run");
    OVERRIDDEN.store(false, Ordering::Relaxed);
    set_limit(Some(256 * 1024));
    assert_eq!(stats().limit, Some(256 * 1024));
    set_limit(None);
    assert_eq!(stats().limit, None);
}

#[test]
fn tracking_ends_with_the_call_even_nested_or_after_a_panic() {
    let _alone = alone();
    assert!(!tracking());
    track(|| {
        assert!(tracking());
        track(|| assert!(tracking()));
        assert!(tracking(), "still inside the outer call");
    });
    assert!(!tracking());
    // Unwound without the panic hook: its message, printed into the
    // harness's buffer, would be charged and freed elsewhere (see the
    // module's documentation). The payload is allocated tracked.
    let before = live();
    let caught = std::panic::catch_unwind(|| {
        track(|| std::panic::resume_unwind(Box::new("a plugin's panic")))
    });
    assert!(caught.is_err());
    assert!(!tracking(), "restored while unwinding");
    drop(caught);
    assert_eq!(live(), before, "all of it freed here");
}

#[test]
fn allocations_of_any_alignment_keep_their_mark_apart() {
    for align in [1, 2, 8, 16, 32, 64, 4096] {
        for size in [0, 1, 15, 16, 100] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let (whole, offset) = outer(layout);
            assert!(offset >= 16 && offset % align == 0, "{layout:?}");
            assert_eq!(whole.size(), size + offset);
            assert_eq!(whole.align(), align);
        }
    }
    // Through the allocator, tracked: aligned, writable, freed whole.
    let _alone = alone();
    let before = live();
    track(|| {
        let layout = Layout::from_size_align(100, 4096).unwrap();
        // SAFETY: a non-zero size; freed with the same layout below.
        unsafe {
            let ptr = std::alloc::alloc(layout);
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % 4096, 0);
            ptr.write_bytes(0xAB, 100);
            assert_eq!(ptr.sub(1).read(), CHARGED);
            std::alloc::dealloc(ptr, layout);
        }
    });
    assert_eq!(live(), before);
}

#[test]
fn growing_and_shrinking_a_tracked_buffer_keeps_the_count_exact() {
    let _alone = alone();
    let before = live();
    let mut buffer = track(|| {
        let mut buffer = Vec::<u8>::with_capacity(16);
        buffer.extend(std::iter::repeat_n(7, 10_000));
        buffer.shrink_to_fit();
        buffer
    });
    assert_eq!(live(), before + buffer.capacity() + HEADER);
    // Grown outside the kernel's calls: the new memory is the shell's.
    buffer.reserve(100_000);
    assert_eq!(live(), before);
}

#[test]
fn the_profile_records_live_allocations_by_site_until_freed() {
    let _alone = alone();
    PROFILE.store(true, Ordering::Relaxed);
    let buffer = track(|| Vec::<u8>::with_capacity(4321));
    let recorded = |sites: &Mutex<Option<HashMap<usize, Site>>>| {
        sites
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|s| s.get(&(buffer.as_ptr() as usize)).map(|(size, _)| *size))
    };
    assert_eq!(recorded(&SITES), Some(4321));
    let at = buffer.as_ptr() as usize;
    drop(buffer);
    let gone = SITES
        .lock()
        .unwrap()
        .as_ref()
        .is_none_or(|s| !s.contains_key(&at));
    PROFILE.store(false, Ordering::Relaxed);
    assert!(gone, "forgotten when freed");
    // The report takes what is recorded.
    report(2);
    assert!(SITES.lock().unwrap().is_none());
}

#[test]
fn the_profile_keeps_what_was_live_at_the_peak_after_it_is_freed() {
    let _alone = alone();
    reset_peak();
    PROFILE.store(true, Ordering::Relaxed);
    let peak = track(|| Vec::<u8>::with_capacity(PEAK_STEP * 3));
    drop(peak);
    let later = track(|| Vec::<u8>::with_capacity(100));
    PROFILE.store(false, Ordering::Relaxed);
    let sizes: Vec<usize> = AT_PEAK
        .lock()
        .unwrap()
        .as_ref()
        .map(|(_, sites)| sites.iter().map(|(size, _)| *size).collect())
        .unwrap_or_default();
    assert!(sizes.contains(&(PEAK_STEP * 3)), "{sizes:?}");
    assert!(!sizes.contains(&100), "{sizes:?}");
    // A new peak starts it anew; the report takes it.
    drop(later);
    reset_peak();
    assert!(AT_PEAK.lock().unwrap().is_none());
    report(2);
}

#[test]
fn a_site_leaves_out_the_budgets_own_frames() {
    // Only this module's frames are PaperPaw's here, and the budget's
    // bookkeeping is never where memory is used.
    let backtrace = Backtrace::force_capture();
    assert_eq!(site_of(&backtrace, 2), "(outside PaperPaw code)");
}

#[test]
fn a_site_names_methods_of_paperpaw_types() {
    let text =
        "   0: <paperpaw_emulator::budget::Budget as core::alloc::global::GlobalAlloc>::alloc
             at ./shells/desktop/src/budget.rs:95:20
   1: <alloc::vec::Vec<u8>>::with_capacity
             at /rustc/library/alloc/src/vec/mod.rs:480:9
   2: <paperpaw_core::Kernel>::start::h0123456789abcdef
             at ./crates/paperpaw-core/src/lib.rs:120:5
   3: paperpaw_emulator::device::build_kernel
             at ./shells/desktop/src/device.rs:1022:9
   4: paperpaw_emu::main
             at ./shells/desktop/src/main.rs:99:5";
    assert_eq!(
        site_in(text, 2),
        "<paperpaw_core::Kernel>::start (lib.rs:120)\n      <- paperpaw_emulator::device::build_kernel (device.rs:1022)"
    );
}

#[test]
fn stats_show_the_heap_in_kib_with_the_budget_where_there_is_one() {
    let with = Stats {
        live: 1536,
        peak: 2048,
        limit: Some(256 * 1024),
        failed: 0,
    };
    assert_eq!(with.to_string(), "heap 1.5 KiB / 256 KiB (peak 2.0 KiB)");
    let without = Stats {
        limit: None,
        ..with
    };
    assert_eq!(without.to_string(), "heap 1.5 KiB (peak 2.0 KiB)");
}

#[test]
fn flat_shared_depth_refuses_actual_allocation_at_full_heap_but_can_be_retained() {
    use paperpaw_core::native_host::flat::{FlatDepth, FlatError};
    let _alone = alone();
    let baseline = stats();
    let depth = track(FlatDepth::try_new).unwrap();
    let before = stats();
    set_limit(Some(before.live));
    let refused = track(FlatDepth::try_new);
    let retained = track(|| depth.try_clone());
    let after = stats();
    set_limit(None);
    assert!(matches!(refused, Err(FlatError::Allocation)));
    assert!(retained.is_some());
    assert!(core::ptr::eq(
        depth.as_depth(),
        retained.as_ref().unwrap().as_depth()
    ));
    assert_eq!(after.failed, before.failed + 1);
    assert_eq!(after.live, before.live);
    let charged = before.live - baseline.live;
    drop(retained);
    drop(depth);
    assert_eq!(live(), baseline.live);
    eprintln!(
        "FlatDepth shared allocation: {charged} budget bytes including header; full-heap retain: 0 bytes"
    );
}
