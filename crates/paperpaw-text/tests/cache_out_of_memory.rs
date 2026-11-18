//! A glyph read from a stored pack, and the anchors of a key, are kept only
//! where the heap gives room: a refusal at any allocation of the cold path
//! answers "not read now" instead of stopping the device (#228, slot 30).
//!
//! The allocator refuses exactly the n-th allocation after it is armed, for
//! every n the cold path makes; an infallible allocation would abort the test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;

use paperpaw_text::{Font, Glyph, GlyphCache, LineMetrics, PackBuilder, PackSource, StoredPack};

struct Refusing;

thread_local! {
    /// Allocations left before the refused one; `None`: not armed.
    static LEFT: Cell<Option<usize>> = const { Cell::new(None) };
    static REFUSED: Cell<bool> = const { Cell::new(false) };
}

fn refuse_now() -> bool {
    LEFT.try_with(|left| match left.get() {
        Some(0) => {
            left.set(None);
            let _ = REFUSED.try_with(|refused| refused.set(true));
            true
        }
        Some(n) => {
            left.set(Some(n - 1));
            false
        }
        None => false,
    })
    .unwrap_or(false)
}

// SAFETY: forwards to the system allocator, or answers null (a refusal),
// which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse_now() {
            return core::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse_now() {
            return core::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if refuse_now() {
            return core::ptr::null_mut();
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

struct Memory(Vec<u8>);

impl PackSource for Memory {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let Ok(at) = usize::try_from(at) else {
            return false;
        };
        match self.0.get(at..at.saturating_add(buf.len())) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

#[allow(clippy::unwrap_used, reason = "the test builds a valid pack")]
fn stored() -> Font {
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    let glyphs = ['a', 'b']
        .into_iter()
        .map(|ch| (ch, Glyph::empty(3.0)))
        .collect();
    builder.add_strike(10.0, metrics, glyphs);
    Font::Stored(StoredPack::open(Rc::new(Memory(builder.build()))).unwrap())
}

#[test]
fn every_refused_allocation_of_a_cold_glyph_reads_as_not_read() {
    let font = stored();
    let mut kept = false;
    for n in 0..64 {
        // A cold cache: its entry list has no room yet, so keeping the
        // first glyph allocates too.
        let mut cache = GlyphCache::new(16 * 1024);
        REFUSED.with(|refused| refused.set(false));
        LEFT.with(|left| left.set(Some(n)));
        let advance = cache.glyph(0, &font, 'b', 10.0).advance;
        LEFT.with(|left| left.set(None));
        // Read or not, the line keeps its measured advance.
        assert_eq!(advance, 3.0, "refused allocation {n}");
        if !REFUSED.with(Cell::get) {
            // The cold path made fewer than n + 1 allocations: all covered.
            kept = true;
            break;
        }
        // Not kept where refused: the next call reads it again.
        let _ = cache.glyph(0, &font, 'b', 10.0);
    }
    assert!(
        kept,
        "the cold path made more allocations than the test covers"
    );
}
