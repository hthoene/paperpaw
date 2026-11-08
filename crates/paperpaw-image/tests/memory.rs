//! The memory decoders take, counted by an allocator of this test: the
//! size a file claims (up to 65535 × 65535) must not decide it, the
//! screen's size and a picture's width may. Small devices have a few
//! hundred KiB; a decoder that asks for more than it needs fails there,
//! and elsewhere fills gigabytes for a file of a few bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::{Checked, Gif, Jpeg, Png};
use paperpaw_image::{Fit, Format, ImageError, decode};

/// Counts what each thread holds, its peak and its largest request;
/// refuses requests above [`REFUSED_ABOVE`] (as a small device would,
/// without the machine running the tests filling gigabytes).
struct Counting;

const REFUSED_ABOVE: usize = 256 << 20;

thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

fn grew(bytes: usize) {
    let _ = LIVE.try_with(|live| {
        live.set(live.get() + bytes);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
    });
}

fn shrank(bytes: usize) {
    let _ = LIVE.try_with(|live| live.set(live.get().saturating_sub(bytes)));
}

fn asked(bytes: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(bytes)));
}

// SAFETY: passes every call on to the system allocator unchanged, or
// refuses it with a null pointer, as `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        asked(layout.size());
        if layout.size() > REFUSED_ABOVE {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout, as given.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        asked(layout.size());
        if layout.size() > REFUSED_ABOVE {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout, as given.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrank(layout.size());
        // SAFETY: allocated by `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        asked(new_size);
        if new_size > REFUSED_ABOVE {
            return std::ptr::null_mut();
        }
        // SAFETY: allocated by `System` with this layout.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            shrank(layout.size());
            grew(new_size);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What decoding `data` returns, the most it held at once on top of
/// what was held before, and its largest single request.
fn measure(
    format: Format,
    data: &[u8],
    fit: Fit,
    color: bool,
) -> (Result<(), ImageError>, Checked, usize, usize) {
    let mut sink = if color {
        Checked::colour()
    } else {
        Checked::gray()
    };
    // Room for rows the sink keeps, taken before counting.
    sink.pixels.reserve(4 << 20);
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    LARGEST.with(|largest| largest.set(0));
    let mut source = data;
    let result = decode(format, &mut source, fit, &mut sink);
    let peak = PEAK.with(Cell::get) - base;
    (result, sink, peak, LARGEST.with(Cell::get))
}

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// The X4's screen.
const SCREEN: Fit = Fit::screen(480, 800);

/// `data` with the 16-bit big-endian size at `at` (height, width) set to
/// 65535 × 65535.
fn huge_jpeg(data: &[u8]) -> Vec<u8> {
    let sof = common::find_marker(data, 0xc0, 0)
        .or_else(|| common::find_marker(data, 0xc2, 0))
        .unwrap();
    let mut out = data.to_vec();
    out[sof + 5..sof + 9].fill(0xff);
    out
}

#[test]
fn huge_baseline_jpegs_take_what_the_screen_needs() {
    for sampling in [&[(1, 1)][..], &[(2, 2), (1, 1), (1, 1)]] {
        // Without its end marker: zeros would be read after it.
        let data = Jpeg::new(64, 64, sampling, 1, false).baseline();
        let data = huge_jpeg(&data[..data.len() - 2]);
        for color in [false, true] {
            let (result, sink, peak, _) = measure(Format::Jpeg, &data, SCREEN, color);
            assert!(result.is_err());
            assert_eq!((sink.width, sink.height), (480, 480));
            assert!(peak < 256 * KIB, "{peak} bytes held");
        }
    }
}

#[test]
fn huge_progressive_jpegs_are_refused_before_taking_memory() {
    let data = huge_jpeg(&Jpeg::new(64, 64, &[(2, 2), (1, 1), (1, 1)], 1, true).progressive());
    for color in [false, true] {
        let (result, _, peak, largest) = measure(Format::Jpeg, &data, SCREEN, color);
        assert_eq!(result, Err(ImageError::OutOfMemory));
        assert!(peak < 256 * KIB, "{peak} bytes held");
        assert!(largest < 256 * KIB, "{largest} bytes asked for");
    }
}

#[test]
fn progressive_coefficients_stay_within_their_limit() {
    // 4000 × 4000 decoded whole: more coefficients than the decoder may
    // keep (8 MiB); it keeps fewer frequencies instead.
    let mut data = Jpeg::new(64, 64, &[(1, 1)], 1, true).progressive();
    let sof = common::find_marker(&data, 0xc2, 0).unwrap();
    data[sof + 5..sof + 9].copy_from_slice(&[0x0f, 0xa0, 0x0f, 0xa0]);
    let (result, sink, peak, _) = measure(Format::Jpeg, &data, Fit::side(100_000), false);
    // The data ends early; nothing was shown.
    assert!(result.is_err());
    assert!(!sink.started);
    assert!(peak <= 8 * MIB + 256 * KIB, "{peak} bytes held");
}

#[test]
fn huge_pngs_take_what_a_row_needs_whatever_their_height() {
    for (color, depth) in [(0, 1), (0, 8), (6, 16)] {
        let mut png = Png::new(1, 1, color, depth, 0);
        png.width = 65536;
        let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&[0; 64], 6);
        png.height = 1;
        let (_, _, one_row, _) = measure(Format::Png, &png.with_zlib(&zlib), SCREEN, true);
        png.height = 65536;
        let (result, sink, all_rows, _) = measure(Format::Png, &png.with_zlib(&zlib), SCREEN, true);
        assert_eq!(result, Err(ImageError::Invalid("truncated PNG data")));
        assert_eq!((sink.width, sink.height), (480, 480));
        assert!(all_rows <= one_row + KIB, "{all_rows} against {one_row}");
        // Two rows of the widest RGBA of 16 bits are 1 MiB.
        assert!(all_rows < 3 * MIB / 2, "{all_rows} bytes held");
    }
}

#[test]
fn large_gif_screens_take_what_a_row_needs() {
    // The largest screen taken (64 megapixels), the picture's data cut
    // short.
    let mut gif = Gif::new(3, 3, 4, 0);
    gif.width = 8192;
    gif.height = 8192;
    let data = gif.encode();
    let (result, _, peak, _) = measure(Format::Gif, &data[..data.len() - 6], SCREEN, true);
    assert!(result.is_err());
    assert!(peak < 256 * KIB, "{peak} bytes held");
}

#[test]
fn interlaced_gif_pictures_larger_than_their_screen_ask_for_nothing() {
    // A picture of 65535 × 65535 on a screen of 16 × 16, a few bytes of
    // file: it would be gathered in a buffer of `w * h` (4 GiB), interlaced.
    // The decoder refuses a picture larger than its screen before it asks
    // for anything.
    let mut gif = Gif::new(2, 2, 4, 0);
    gif.width = 16;
    gif.height = 16;
    gif.frame_width = 65535;
    gif.frame_height = 65535;
    gif.interlaced = true;
    let data = gif.encode();
    assert!(data.len() < 100);
    let (_, _, _, largest) = measure(Format::Gif, &data, SCREEN, false);
    assert!(largest < MIB, "{largest} bytes asked for at once");
}
