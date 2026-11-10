//! Memory refused while entities are decoded is an error
//! ([`TryReserveError`]), never an abort and never a partly decoded value:
//! the global allocator here refuses allocations while a test thread has
//! armed it (per thread, undone on drop, so other tests never see one).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_xml::{Token, Tokenizer, decode, try_decode};

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

/// An entity-heavy value, as OPDS feeds have them (titles, summaries, links).
const HEAVY: &str =
    "Tom &amp; Jerry &#8220;Caf&eacute;&#8221; &mdash; &lt;b&gt; &#x1F600; &unknown; &";

/// Every allocation `try_decode` makes, refused in turn, is the error;
/// with room it is exactly `decode`'s text.
#[test]
fn try_decode_refused_at_every_allocation_is_an_error() {
    let expected = decode(HEAVY).into_owned();
    let mut left = 0;
    loop {
        let (decoded, refused) = armed_after(left, || try_decode(HEAVY).map(|t| t.len()));
        match decoded {
            Err(_) => assert!(refused > 0, "an error without a refusal at {left}"),
            Ok(len) => {
                assert_eq!(refused, 0, "a refusal swallowed at {left}");
                assert_eq!(len, expected.len());
                break;
            }
        }
        left += 1;
        assert!(left < 16, "try_decode allocates without end");
    }
    assert_eq!(try_decode(HEAVY).unwrap(), expected);
    // Text without a reference is borrowed: nothing to refuse.
    let (plain, refused) = armed_after(0, || try_decode("plain text").is_ok());
    assert!(plain && refused == 0);
}

/// `Attrs::try_get` on a feed's start tags, every allocation refused in
/// turn: an error, or the same value as `get`, never a cut one.
#[test]
fn attribute_values_refused_at_every_allocation_are_errors() {
    let feed = alloc_feed();
    let mut checked = 0;
    Tokenizer::parse(feed.as_bytes(), &mut |token| {
        let Token::Start { attrs, .. } = token else {
            return;
        };
        let Some(want) = attrs.get("title").map(|t| t.into_owned()) else {
            return;
        };
        let mut left = 0;
        loop {
            let (got, refused) =
                armed_after(left, || attrs.try_get("title").map(|v| v.map(|t| t.len())));
            match got {
                Err(_) => assert!(refused > 0, "an error without a refusal at {left}"),
                Ok(len) => {
                    assert_eq!(refused, 0, "a refusal swallowed at {left}");
                    assert_eq!(len, Some(want.len()));
                    break;
                }
            }
            left += 1;
            assert!(left < 16, "try_get allocates without end");
        }
        assert_eq!(attrs.try_get("title").unwrap().unwrap(), want);
        assert_eq!(attrs.try_get("missing").unwrap(), None);
        checked += 1;
    });
    assert_eq!(checked, 2);
}

fn alloc_feed() -> String {
    format!(
        "<feed><entry><link title=\"{HEAVY}\" href=\"a&amp;b\"/></entry>\
         <entry><link title=\"plain\"/></entry></feed>"
    )
}
