//! The report and the verdicts when the heap refuses: each allocation they
//! make is refused in turn. An allocation that would abort (an infallible
//! `format!`, `String::from`, `push`) ends the whole test binary; a refused
//! one must come back as `Error::NoMemory`, never as a panic or a half text.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "invalid fixture setup fails tests"
)]
use paperpaw_plugin_diagnostics::steps::{
    About, BatteryReading, Buttons, CardTest, Mark, MemoryReading, Outcome, Results, Step, battery,
    memory,
};
use paperpaw_sdk::native::Error;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

std::thread_local! {
    /// The allocation to refuse, counted from 1; 0 turns refusing off.
    static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct Failing;

fn refuse() -> bool {
    let at = REFUSE_AT.try_with(Cell::get).unwrap_or(0);
    if at == 0 {
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

#[global_allocator]
static ALLOCATOR: Failing = Failing;

/// Runs `run` with the `at`-th allocation refused (`usize::MAX`: none refused,
/// only counted); returns its result and how many allocations it made.
fn armed<T>(at: usize, run: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|count| count.set(0));
    REFUSE_AT.with(|refuse| refuse.set(at));
    let result = run();
    REFUSE_AT.with(|refuse| refuse.set(0));
    (result, COUNT.with(Cell::get))
}

/// Builds test input while nothing is refused or counted.
fn fixture<T>(build: impl FnOnce() -> T) -> T {
    let at = REFUSE_AT.with(|refuse| refuse.replace(0));
    let value = build();
    REFUSE_AT.with(|refuse| refuse.set(at));
    value
}

/// Refuses each allocation of `run` in turn: it must be `Ok` with none
/// refused and `Err(NoMemory)` (or, where an allocation is optional, `Ok`)
/// with one refused.
fn sweep<T: std::fmt::Debug>(name: &str, run: impl Fn() -> Result<T, Error>) {
    let (whole, count) = armed(usize::MAX, &run);
    whole.unwrap_or_else(|error| panic!("{name}: {error:?} with nothing refused"));
    assert!(
        count > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
    for at in 1..=count {
        let (result, _) = armed(at, &run);
        assert!(
            matches!(result, Ok(_) | Err(Error::NoMemory)),
            "{name}: refusing allocation {at} gave {result:?}"
        );
    }
}

fn marked() -> Results {
    let mut results = Results::default();
    for (i, step) in Step::ALL.into_iter().enumerate() {
        let note = if i % 2 == 0 { "a note" } else { "" };
        results.set(step, Mark::Done(Outcome::Pass, note.to_string()));
    }
    results.set(Step::Sleep, Mark::Asleep);
    results
}

#[test]
fn the_report_refuses_cleanly() {
    let results = marked();
    let about = About {
        name: "Device",
        id: "device",
        dpi: 220,
        format: "Gray4",
        version: "1.0.9",
        when: Some("2026-10-23T10:00:00.000Z"),
    };
    sweep("report", || results.report(&about));
}

#[test]
fn reading_a_report_refuses_cleanly() {
    let text =
        "PaperPaw device check\nscreen: pass (16 gray levels)\nbuttons: fail\nsleep: asleep\n";
    sweep("parse", || Results::parse(text));
}

#[test]
fn the_verdicts_refuse_cleanly() {
    sweep("battery", || {
        battery(
            Some(BatteryReading {
                percent: 85,
                millivolts: Some(4012),
            }),
            [true, true],
        )
    });
    sweep("battery, none", || battery(None, [false, false]));
    sweep("memory", || {
        memory(Some(MemoryReading {
            heap_free: 100 * 1024,
            heap_size: 256 * 1024,
            stack_free: Some(4096),
            stack_size: Some(16384),
        }))
    });
    sweep("card", || {
        CardTest {
            bytes: 128 * 1024,
            write_us: Some(500_000),
            read_us: Some(125_000),
            error: None,
        }
        .verdict()
    });
    sweep("card error", || {
        let test = fixture(|| CardTest {
            bytes: 0,
            write_us: None,
            read_us: None,
            error: Some("read back differs".to_string()),
        });
        test.verdict()
    });
    sweep("buttons", || {
        let names = fixture(|| vec!["up".to_string(), "down".to_string()]);
        let mut buttons = Buttons::new(names);
        buttons.press("up", false)?;
        buttons.press("down", true)?;
        buttons.verdict()
    });
}
