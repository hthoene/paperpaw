//! Keeping the bands of a frame as they are sent takes the heap of their
//! packed bytes and nothing of a band's size beside them, while the rest
//! of the frame is drawn: the X4 has about 240 KiB of heap in all, a band
//! of its frame is 9,600 bytes, and a frame's drawing is where the heap is
//! at its fullest (#248).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_platform::{FrameView, PixelFormat};
use paperpaw_x4::sent::Sent;

/// Counts the bytes a thread has allocated now, and the most since its peak
/// was reset: each test counts its own thread's, so neither the other test
/// nor the harness printing beside it is in its figures.
struct Counting;

thread_local! {
    static NOW: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: passes every call on to the system's allocator; the counts are
// plain cells of the thread, with nothing to allocate or to drop.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as the caller asked.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let _ = NOW.try_with(|now| {
                now.set(now.get() + layout.size());
                let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now.get())));
            });
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) };
        let _ = NOW.try_with(|now| now.set(now.get().saturating_sub(layout.size())));
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most this thread's heap grows above where it is while `run` runs,
/// where it ends above it, and what `run` gives.
fn growth<T>(run: impl FnOnce() -> T) -> (usize, usize, T) {
    let before = NOW.get();
    PEAK.set(before);
    let out = run();
    (PEAK.get() - before, NOW.get().saturating_sub(before), out)
}

/// The X4's frame: 480 by 800 at a bit a pixel, in five bands.
const STRIDE: usize = 60;
const ROWS: u16 = 160;
const BAND: usize = STRIDE * ROWS as usize;
const BANDS: u16 = 5;
/// What the list of the bands and the like may take.
const SMALL: usize = 512;

fn plenty() -> usize {
    usize::MAX / 2
}

/// A band of a page of text: white, with rows of mixed bytes.
fn text(band: u16) -> Vec<u8> {
    (0..BAND as u32)
        .map(|i| {
            let (row, column) = (i / STRIDE as u32, i % STRIDE as u32);
            if row % 8 < 3 && column > 6 && column < 52 {
                ((i + u32::from(band) * 977).wrapping_mul(2_654_435_761) >> 13) as u8
            } else {
                0xFF
            }
        })
        .collect()
}

/// A band of a dithered picture: nothing in it packs.
fn picture(band: u16) -> Vec<u8> {
    (0..BAND as u32)
        .map(|i| ((i + u32::from(band) * 977).wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect()
}

/// Keeps a frame of `bands`: the most the heap grew while keeping it and
/// what the kept frame holds, then sends it again: the most that took, and
/// whether the bands came out as they went in.
fn frame(bands: &[Vec<u8>]) -> (usize, usize, usize, bool) {
    let mut sent = Sent::new(plenty);
    let (peak, held, ()) = growth(|| {
        for (n, band) in bands.iter().enumerate() {
            let view = FrameView::new(480, ROWS, PixelFormat::Mono1, STRIDE, band);
            sent.keep(n as u16 * ROWS, &view);
        }
    });
    let mut same = true;
    let mut seen = 0;
    let (again, _, kept) = growth(|| {
        sent.replay(&mut |top, rows| {
            let n = usize::from(top / ROWS);
            same &= bands.get(n).is_some_and(|band| band[..] == *rows.data);
            seen += 1;
        })
    });
    (peak, held, again, kept && same && seen == bands.len())
}

#[test]
fn a_page_of_text_kept_takes_what_it_packs_to_and_no_band_beside_it() {
    let bands: Vec<Vec<u8>> = (0..BANDS).map(text).collect();
    let (peak, held, again, same) = frame(&bands);
    assert!(same);
    // Packed, the page is well under half of its 48,000 bytes ...
    assert!(held < usize::from(BANDS) * BAND / 2, "{held}");
    // ... and keeping it never took more than what is kept in the end.
    assert!(peak <= held + SMALL, "peak {peak}, held {held}");
    // Sending it again takes a band to unpack into, and no more.
    assert!(again <= BAND + SMALL, "{again}");
}

#[test]
fn a_picture_kept_takes_its_bands_and_not_one_more() {
    let bands: Vec<Vec<u8>> = (0..BANDS).map(picture).collect();
    let (peak, held, again, same) = frame(&bands);
    assert!(same);
    let packed = BAND + BAND / 128 + 1;
    assert!(held <= usize::from(BANDS) * packed + SMALL, "{held}");
    assert!(peak <= held + SMALL, "peak {peak}, held {held}");
    assert!(again <= BAND + SMALL, "{again}");
}
