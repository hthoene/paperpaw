//! Memory refused while JSON streams in is an error
//! ([`OutOfMemory`]), never an abort, and never a cut or partial event:
//! the global allocator here refuses allocations while a test thread has
//! armed it (per thread, undone on drop, so other tests never see one).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_json::stream::{Event, OutOfMemory, Parser};

/// This thread's refusals: while `armed`, allocations after `left` more
/// are refused; `refused` counts them.
struct Arm {
    armed: Cell<bool>,
    left: Cell<usize>,
    refused: Cell<usize>,
}

thread_local! {
    static ARM: Arm = const {
        Arm {
            armed: Cell::new(false),
            left: Cell::new(0),
            refused: Cell::new(0),
        }
    };
}

fn allowed() -> bool {
    ARM.try_with(|arm| {
        if !arm.armed.get() {
            return true;
        }
        match arm.left.get() {
            0 => {
                arm.refused.set(arm.refused.get() + 1);
                false
            }
            left => {
                arm.left.set(left - 1);
                true
            }
        }
    })
    .unwrap_or(true)
}

struct Refusing;

// SAFETY: forwards to the system allocator, or refuses with null, which
// `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !allowed() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout, passed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by `System` above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if !allowed() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's block and layout, passed on.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Disarms on drop, also when a test panics.
struct Armed;

impl Drop for Armed {
    fn drop(&mut self) {
        ARM.with(|arm| arm.armed.set(false));
    }
}

/// Runs `f` with this thread's allocations after the first `left`
/// refused; also tells how many were refused.
fn armed_after<T>(left: usize, f: impl FnOnce() -> T) -> (T, usize) {
    ARM.with(|arm| {
        arm.refused.set(0);
        arm.left.set(left);
        arm.armed.set(true);
    });
    let out = {
        let _armed = Armed;
        f()
    };
    (out, ARM.with(|arm| arm.refused.get()))
}

/// An event as owned text, made after the parser reported it. Kept in a
/// buffer reserved before arming, so the test's own record allocates
/// nothing while armed.
fn describe(event: Event<'_>, out: &mut String) {
    let (kind, path, text) = match event {
        Event::Open(path) => ("open", path, ""),
        Event::Close(path) => ("close", path, ""),
        Event::Value(path, text) => ("value", path, text),
    };
    out.push_str(kind);
    for key in path {
        out.push('/');
        out.push_str(key);
    }
    out.push('=');
    out.push_str(text);
    out.push('\n');
}

/// Keys, arrays in objects, escapes, a surrogate pair and a lone half,
/// invalid UTF-8, numbers and a value cut at the end of the text.
const JSON: &[u8] =
    b"{\"feed\": {\"title\": \"Caf\\u00e9 \\ud83d\\ude00 \\ud800!\", \"entries\": [\
    {\"id\": 1, \"tags\": [\"a\", \"b\\n\"], \"ok\": true},\
    {\"id\": 2.5e3, \"name\": \"bad \xff\xfe here\", \"none\": null},\
    [[[\"deep\"]]]\
  ], \"k\\u0041y\": -7}, \"last\": 42";

fn read(
    feed: impl Fn(&mut Parser, &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory>,
    out: &mut String,
) -> Result<(), OutOfMemory> {
    let mut parser = Parser::default();
    feed(&mut parser, &mut |event| describe(event, out))?;
    parser.try_finish(&mut |event| describe(event, out))
}

#[test]
fn every_allocation_refused_while_parsing_is_an_error_with_whole_events_before_it() {
    let feed_whole =
        |parser: &mut Parser, emit: &mut dyn FnMut(Event<'_>)| parser.try_feed(JSON, emit);
    let mut whole = String::new();
    read(feed_whole, &mut whole).expect("reads with room");
    // The infallible form reports the same.
    let mut infallible = String::new();
    let mut parser = Parser::default();
    parser.feed(JSON, &mut |event| describe(event, &mut infallible));
    parser.finish(&mut |event| describe(event, &mut infallible));
    assert_eq!(infallible, whole);
    assert!(
        whole.contains("value/feed/title=Café 😀 \u{fffd}!\n"),
        "{whole}"
    );
    assert!(
        whole.contains("value/feed/entries/[]/name=bad \u{fffd}\u{fffd} here\n"),
        "{whole}"
    );
    assert!(whole.ends_with("value/last=42\n"), "{whole}");
    // Fed whole, and fed a byte at a time.
    let feed_bytes = |parser: &mut Parser, emit: &mut dyn FnMut(Event<'_>)| {
        JSON.iter()
            .try_for_each(|b| parser.try_feed(core::slice::from_ref(b), emit))
    };
    for (name, feed) in [
        (
            "whole",
            &feed_whole
                as &dyn Fn(&mut Parser, &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory>,
        ),
        ("bytes", &feed_bytes),
    ] {
        for left in 0.. {
            assert!(left < 10_000, "{name}: parsing never ends");
            let mut seen = String::with_capacity(4 * whole.len());
            let (read, refused) = armed_after(left, || read(feed, &mut seen));
            if refused == 0 {
                assert!(left > 5, "{name}: parsing allocates ({left})");
                assert_eq!(read, Ok(()));
                assert_eq!(seen, whole, "{name}");
                break;
            }
            assert_eq!(refused, 1, "{name}: stops at the first refusal ({left})");
            assert!(read.is_err(), "{name}: allocation {left}");
            // What was reported is a whole start of what a parse with room
            // reports: no cut or partial event.
            assert!(
                whole.starts_with(&seen),
                "{name}: allocation {left}: {seen}"
            );
        }
    }
}
