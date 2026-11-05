//! Host checks of the prototype (#228): both paths give the same result,
//! what each allocates, and a micro-benchmark (run it with
//! `cargo test --release -p paperpaw-flat-proto -- --nocapture bench`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    unsafe_code,
    reason = "tests"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use paperpaw_flat_proto::bench::{self, Probe};
use paperpaw_flat_proto::flat::{self, FLAT, FlatGfx, FlatPlugin, HostCtx};
use paperpaw_flat_proto::rusty::{self, RUSTY, RustyGfx};
use paperpaw_flat_proto::widget::{self, Look};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat, Point, Rect};
use paperpaw_sdk::Command;
use paperpaw_text::{Font, GlyphCache};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static COUNTING: Counting = Counting;

const PACK: &[u8] = include_bytes!("../../paperpaw-text/testdata/pack.ppfp.v1");
const INK: Color = Color { r: 0, g: 0, b: 0 };

fn frame() -> Framebuffer {
    Framebuffer::new(240, 96, PixelFormat::Gray8).unwrap()
}

fn pixels(frame: &Framebuffer) -> Vec<u8> {
    let view = frame.view();
    (0..96u16)
        .flat_map(|y| (0..240u16).map(move |x| (x, y)))
        .map(|(x, y)| view.pixel(x, y).r)
        .collect()
}

fn look() -> Look {
    Look {
        ink: INK,
        paper: Color { r: 255, g: 255, b: 255 },
        faint: Color { r: 110, g: 110, b: 110 },
    }
}

#[test]
fn both_paths_draw_the_same_widget() {
    let font = Font::from_data(PACK, &[]).unwrap();
    for (percent, charging) in [(87u8, true), (5, false), (100, true), (0, false)] {
        let mut a = frame();
        let mut b = frame();
        let mut cache_a = GlyphCache::new(8 * 1024);
        let mut cache_b = GlyphCache::new(8 * 1024);
        {
            let mut canvas = a.canvas();
            canvas.clear(Color::WHITE);
            let mut g = RustyGfx { table: &RUSTY, canvas: &mut canvas, font: &font, cache: &mut cache_a };
            widget::battery(&mut g, &look(), Rect::new(8, 8, 120, 60), percent, charging);
        }
        {
            let mut canvas = b.canvas();
            canvas.clear(Color::WHITE);
            let mut ctx = HostCtx { canvas: &mut canvas, font: &font, cache: &mut cache_b };
            let mut g = FlatGfx { table: &FLAT, ctx: (&raw mut ctx).cast::<c_void>() };
            widget::battery(&mut g, &look(), Rect::new(8, 8, 120, 60), percent, charging);
        }
        let (pa, pb) = (pixels(&a), pixels(&b));
        assert!(pa.iter().any(|&p| p < 255), "something is drawn");
        assert_eq!(pa, pb, "{percent} % charging {charging}");
    }
}

#[test]
fn both_paths_deliver_the_same_command() {
    let handler = rusty::plugin();
    let mut a = Vec::new();
    handler(&rusty::Event { kind: 1, payload: "Dune" }, &mut a);
    let mut plugin = FlatPlugin::new();
    let table = plugin.table();
    let mut b = Vec::new();
    flat::deliver(&table, 1, "Dune", &mut b);
    assert_eq!(a, b);
    assert!(matches!(&a[0], Command::Notify(text) if text == "Opened: Dune"));
    // Other kinds say nothing; a payload too long for the buffer is dropped.
    let mut c = Vec::new();
    flat::deliver(&table, 2, "x", &mut c);
    flat::deliver(&table, 1, &"y".repeat(200), &mut c);
    assert!(c.is_empty());
}

#[test]
fn a_flat_call_with_bad_arguments_is_an_error_not_a_crash() {
    let font = Font::from_data(PACK, &[]).unwrap();
    let mut f = frame();
    let mut cache = GlyphCache::new(8 * 1024);
    let mut canvas = f.canvas();
    let mut ctx = HostCtx { canvas: &mut canvas, font: &font, cache: &mut cache };
    let ptr = (&raw mut ctx).cast::<c_void>();
    let bad = [0xFFu8, 0xFE, 0x80];
    // SAFETY: valid pointers and lengths; the bytes are not UTF-8.
    unsafe {
        assert_eq!((FLAT.text_measure)(ptr, 0, 14 * 256, bad.as_ptr(), bad.len()), -2);
        assert_eq!((FLAT.text_measure)(ptr, 9, 14 * 256, bad.as_ptr(), 0), -1);
    }
}

/// Allocations of one call of each kind.
fn allocs(call: impl FnOnce()) -> (usize, usize) {
    let (a, l) = (ALLOCS.load(Ordering::Relaxed), LIVE.load(Ordering::Relaxed));
    call();
    (
        ALLOCS.load(Ordering::Relaxed) - a,
        LIVE.load(Ordering::Relaxed).saturating_sub(l),
    )
}

#[test]
fn the_calls_allocate_the_same_or_less_than_the_rusty_ones() {
    let font = Font::from_data(PACK, &[]).unwrap();
    let mut f = frame();
    let mut cache = GlyphCache::new(8 * 1024);
    let mut canvas = f.canvas();
    let mut ctx = HostCtx { canvas: &mut canvas, font: &font, cache: &mut cache };
    let ptr = (&raw mut ctx).cast::<c_void>();
    // Warm: the glyph cache fills on first use.
    // SAFETY: valid.
    unsafe {
        (FLAT.text_draw)(ptr, 0, 14 * 256, 0xFF00_0000, 0, 30, b"Hello".as_ptr(), 5);
    }
    let rect = allocs(|| unsafe { (FLAT.fill_rect)(ptr, 1, 1, 10, 10, 0xFF00_0000) });
    let line = allocs(|| unsafe { (FLAT.draw_line)(ptr, 1, 1, 50, 20, 2, 0xFF00_0000) });
    let measure = allocs(|| unsafe {
        (FLAT.text_measure)(ptr, 0, 14 * 256, b"Hello".as_ptr(), 5);
    });
    let draw = allocs(|| unsafe {
        (FLAT.text_draw)(ptr, 0, 14 * 256, 0xFF00_0000, 0, 30, b"Hello".as_ptr(), 5);
    });
    println!("flat allocations (count, live bytes): rect {rect:?} line {line:?} measure {measure:?} draw {draw:?}");
    assert_eq!(rect.0, 0);
    assert_eq!(line.0, 0);
    assert_eq!(measure.0, 0);

    let handler = rusty::plugin();
    let mut out = Vec::new();
    out.reserve(4);
    let rusty_deliver = allocs(|| handler(&rusty::Event { kind: 1, payload: "Dune" }, &mut out));
    out.clear();
    let mut plugin = FlatPlugin::new();
    let table = plugin.table();
    let mut out = Vec::new();
    out.reserve(4);
    let flat_deliver = allocs(|| flat::deliver(&table, 1, "Dune", &mut out));
    println!("deliver allocations (count, live bytes): rusty {rusty_deliver:?} flat {flat_deliver:?}");
    assert_eq!(rusty_deliver.0, flat_deliver.0);
    let _ = Point::new(0, 0);
}

#[test]
fn bench_print_the_table() {
    let font = Font::from_data(PACK, &[]).unwrap();
    let mut f = frame();
    let mut cache = GlyphCache::new(8 * 1024);
    let mut canvas = f.canvas();
    let start = Instant::now();
    let probe = Probe {
        micros: || {
            use std::sync::OnceLock;
            static START: OnceLock<Instant> = OnceLock::new();
            START.get_or_init(Instant::now).elapsed().as_micros() as u64
        },
        heap_used: || LIVE.load(Ordering::Relaxed),
    };
    let lines = bench::run(&probe, &font, &mut cache, &mut canvas, 100, bench::Order::RustyFirst);
    println!("host x86-64, release only counts (took {:?}):", start.elapsed());
    for l in lines {
        println!(
            "{:15} n={:7} rusty {:8.1} ns/call  flat {:8.1} ns/call  ratio {:.2}  heap rusty {:+} flat {:+}",
            l.name,
            l.iterations,
            l.rusty_us as f64 * 1000.0 / f64::from(l.iterations),
            l.flat_us as f64 * 1000.0 / f64::from(l.iterations),
            l.flat_us as f64 / l.rusty_us.max(1) as f64,
            l.rusty_heap,
            l.flat_heap
        );
    }
}

#[test]
fn print_the_battery_widget_call_counts() {
    let mut tally = paperpaw_flat_proto::widget::Tally(Default::default());
    paperpaw_flat_proto::widget::battery(
        &mut tally,
        &look(),
        paperpaw_platform::Rect::new(8, 8, 120, 60),
        87,
        true,
    );
    println!("battery widget calls: {:?}", tally.0);
}
