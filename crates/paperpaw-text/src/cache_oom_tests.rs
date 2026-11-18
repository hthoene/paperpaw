//! Refused allocations on the cold path (#228, slot 30): a glyph with
//! pixels is counted only once it is kept, and anchors are answered but
//! not kept. The allocator refuses the n-th allocation after it is armed.

extern crate std;

use super::*;
use crate::anchors::{self, Anchor, Anchors};
use crate::{BitmapFont, LineMetrics, PackBuilder, PackSource, StoredPack};
use core::alloc::{GlobalAlloc, Layout};
use std::alloc::System;
use std::cell::Cell;

struct Refusing;

std::thread_local! {
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

/// `f` with the `n`-th allocation refused; whether a refusal happened.
fn refusing<T>(n: usize, f: impl FnOnce() -> T) -> (T, bool) {
    REFUSED.with(|refused| refused.set(false));
    LEFT.with(|left| left.set(Some(n)));
    let result = f();
    LEFT.with(|left| left.set(None));
    (result, REFUSED.with(Cell::get))
}

/// A 2 x 2 glyph with pixels (coverage, two pixels a byte).
fn inked() -> Glyph {
    let mut glyph = Glyph::empty(4.0);
    glyph.width = 2;
    glyph.height = 2;
    glyph.data = alloc::vec![0xff; 2];
    glyph
}

#[test]
fn a_glyph_with_pixels_is_counted_only_once_kept() {
    let size = inked().byte_size();
    let mut covered = false;
    for n in 0..8 {
        let mut cache = Lru::new(4096);
        let glyph = inked();
        let (kept, refused) = refusing(n, || cache.cached(0, 'a', 10.0, |_| Some(glyph)).is_some());
        if !refused {
            assert!(kept);
            assert_eq!((cache.bytes(), cache.entries.len()), (size, 1));
            covered = true;
            break;
        }
        // Not kept, not counted, and read again next time.
        assert!(!kept, "refused allocation {n}");
        assert_eq!((cache.bytes(), cache.entries.len()), (0, 0));
        assert!(cache.cached(0, 'a', 10.0, |_| Some(inked())).is_some());
        assert_eq!((cache.bytes(), cache.entries.len()), (size, 1));
    }
    assert!(covered);
}

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

fn anchors_of(ch: char) -> Anchors {
    let mut anchors = Anchors::NONE;
    anchors.push(Anchor {
        class: 0,
        x: (u32::from(ch) % 100) as i8,
        y: 1,
    });
    anchors
}

#[test]
fn anchors_are_answered_but_not_kept_where_the_heap_refuses() {
    let chars = ['\u{5d0}', '\u{600}'];
    let mut builder = PackBuilder::new();
    let glyphs = chars.iter().map(|&c| (c, Glyph::empty(1.0))).collect();
    builder.add_strike(10.0, LineMetrics::fallback(10.0), glyphs);
    let keys = alloc::vec![chars.iter().map(|&c| (c, anchors_of(c))).collect()];
    builder.table(BitmapFont::ANCHORS, 1, &anchors::build(&keys));
    let Ok(pack) = StoredPack::open(alloc::rc::Rc::new(Memory(builder.build()))) else {
        panic!("the test's pack opens");
    };
    let font = Font::Stored(pack);
    let mut covered = false;
    for n in 0..8 {
        let mut cache = Lru::new(4096);
        let (anchors, refused) = refusing(n, || cache.anchors(0, &font, chars[1], 10.0));
        assert_eq!(anchors, anchors_of(chars[1]), "refused allocation {n}");
        if !refused {
            assert_eq!(cache.anchored.len(), 1);
            covered = true;
            break;
        }
        assert!(cache.anchored.is_empty());
        // Room again: kept from now on.
        assert_eq!(
            cache.anchors(0, &font, chars[1], 10.0),
            anchors_of(chars[1])
        );
        assert_eq!(cache.anchored.len(), 1);
    }
    assert!(covered);
}
