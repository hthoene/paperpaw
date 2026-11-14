//! The test image's high-water log (#2460, Senior's conditions on #2462;
//! `paperpaw_x4::high_water`): every 30 s a line on the serial log with the
//! stack's room left at its deepest since start, the heap's free bytes and the
//! largest free block; the lows are kept in RTC memory (a reset keeps it) when
//! one falls, and before a sleep, and the next start says what the run
//! before ended with. Read off the device by the owner after the tests
//! (`docs/device-checklist` G7, S1-S8): "stack left" is the figure the band's
//! room (`paperpaw_x4::stack_band::RESERVE`) is judged by.
//!
//! The sample is dear (a scan of the painted stack, a search for the largest
//! block), so the main loop asks only when one is due: a comparison of the
//! clock, no polling of anything else.

use paperpaw_x4::high_water::{Sample, Tracker, WORDS, seal, unseal};

/// The lows kept through a reset.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut KEPT: [u32; WORDS] = [0; WORDS];

/// The tracker of this run (one core, only the main loop and `before_sleep`,
/// which runs in it, touch it).
static mut TRACKER: Option<Tracker> = None;

fn tracker() -> &'static mut Tracker {
    // SAFETY: one core; the main loop and the sleep it calls are the only
    // callers and never run at once.
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(TRACKER);
        slot.get_or_insert_with(|| Tracker::new(crate::micros() / 1000))
    }
}

fn read() -> Sample {
    let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let (stack_left, _) = crate::stack::left();
    Sample {
        stack_left: to_u32(stack_left),
        heap_free: to_u32(esp_alloc::HEAP.free()),
        largest: to_u32(crate::largest_block()),
    }
}

fn keep(low: Sample) {
    // SAFETY: as `tracker`.
    unsafe { core::ptr::addr_of_mut!(KEPT).write_volatile(seal(low)) };
}

/// At start: says what the run before ended with (the lows it kept), if it
/// kept any, and starts this run's tracker.
pub fn start() {
    // SAFETY: as `tracker`; read once at start.
    let kept = unseal(unsafe { core::ptr::addr_of!(KEPT).read_volatile() });
    if let Some(low) = kept {
        log::info!(
            "high-water: the run before: stack left {} B, heap free {} B, largest block {} B",
            low.stack_left,
            low.heap_free,
            low.largest
        );
    }
    let _ = tracker();
}

/// The main loop's turn: a sample and a log line when one is due.
pub fn step(now_ms: u64) {
    let tracker = tracker();
    if !tracker.due(now_ms) {
        return;
    }
    let sample = read();
    let tick = tracker.sampled(now_ms, sample);
    if tick.log {
        let low = tracker.low().unwrap_or(sample);
        log::info!(
            "high-water: stack left {} B (lowest {}), heap free {} B (lowest {}), largest block \
             {} B (lowest {})",
            sample.stack_left,
            low.stack_left,
            sample.heap_free,
            low.heap_free,
            sample.largest,
            low.largest
        );
    }
    if tick.keep
        && let Some(low) = tracker.low()
    {
        keep(low);
    }
}

/// Before a sleep (not a nap): a fresh sample, kept when a figure is lower
/// than what is kept.
pub fn before_sleep() {
    if let Some(low) = tracker().before_sleep(read()) {
        keep(low);
    }
}
