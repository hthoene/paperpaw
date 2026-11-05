//! The same work through both paths, timed, and what each call leaves on
//! the heap.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::hint::black_box;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect};
use paperpaw_text::{Font, GlyphCache};

use crate::flat::{self, FLAT, FlatGfx, FlatPlugin, HostCtx};
use crate::rusty::{self, RUSTY, RustyGfx};
use crate::widget::{self, Look};

/// What the caller measures with.
pub struct Probe {
    /// Microseconds, any origin.
    pub micros: fn() -> u64,
    /// Bytes on the heap now.
    pub heap_used: fn() -> usize,
}

/// One kind of call, through each path.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub name: &'static str,
    pub iterations: u32,
    /// Total microseconds for `iterations` calls.
    pub rusty_us: u64,
    pub flat_us: u64,
    /// Bytes still on the heap after one call (its result kept).
    pub rusty_heap: i64,
    pub flat_heap: i64,
}

const INK: Color = Color { r: 0, g: 0, b: 0 };
const TEXT: &str = "Battery 87 %";

fn timed(probe: &Probe, iterations: u32, mut call: impl FnMut()) -> u64 {
    for _ in 0..8 {
        call();
    }
    let start = (probe.micros)();
    for _ in 0..iterations {
        call();
    }
    (probe.micros)().saturating_sub(start)
}

/// Runs every pair of calls `scale` times as often as the base counts.
pub fn run(
    probe: &Probe,
    font: &Font,
    cache: &mut GlyphCache,
    canvas: &mut Canvas<'_>,
    scale: u32,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let table = black_box(&RUSTY);
    let flat_table = black_box(&FLAT);
    let look = Look {
        ink: INK,
        paper: Color { r: 255, g: 255, b: 255 },
        faint: Color { r: 110, g: 110, b: 110 },
    };
    let body = Rect::new(8, 8, 120, 60);

    // (a) a rectangle
    let n = 2000 * scale;
    let rusty_us = timed(probe, n, || {
        (table.fill_rect)(canvas, black_box(10), 10, 30, 12, INK);
    });
    let mut ctx = HostCtx { canvas, font, cache };
    let ptr: *mut c_void = (&raw mut ctx).cast();
    // SAFETY: the table and its own context.
    let flat_us = timed(probe, n, || unsafe {
        (flat_table.fill_rect)(ptr, black_box(10), 10, 30, 12, 0xFF00_0000);
    });
    lines.push(Line { name: "fill_rect", iterations: n, rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (a) a line
    let n = 500 * scale;
    let canvas = &mut *ctx.canvas;
    let rusty_us = timed(probe, n, || {
        (table.draw_line)(canvas, Point::new(black_box(5), 5), Point::new(90, 40), 2, INK);
    });
    // SAFETY: as above.
    let flat_us = timed(probe, n, || unsafe {
        (flat_table.draw_line)(ptr, black_box(5), 5, 90, 40, 2, 0xFF00_0000);
    });
    lines.push(Line { name: "draw_line", iterations: n, rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (b) measuring text
    let n = 300 * scale;
    let rusty_us = timed(probe, n, || {
        black_box((table.measure)(font, black_box(14.0), TEXT));
    });
    // SAFETY: as above; the text is valid.
    let flat_us = timed(probe, n, || unsafe {
        black_box((flat_table.text_measure)(ptr, 0, black_box(14 * 256), TEXT.as_ptr(), TEXT.len()));
    });
    lines.push(Line { name: "text_measure", iterations: n, rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (b) drawing text
    let n = 60 * scale;
    let canvas = &mut *ctx.canvas;
    let cache = &mut *ctx.cache;
    let rusty_us = timed(probe, n, || {
        let style = paperpaw_text::TextStyle::new(font, 0, black_box(14.0), INK);
        black_box((table.draw_text)(canvas, cache, &style, 10.0, 40, TEXT));
    });
    // SAFETY: as above.
    let flat_us = timed(probe, n, || unsafe {
        black_box((flat_table.text_draw)(ptr, 0, black_box(14 * 256), 0xFF00_0000, 10 * 256, 40, TEXT.as_ptr(), TEXT.len()));
    });
    lines.push(Line { name: "text_draw", iterations: n, rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (c) an event to a plugin, a command back
    let n = 2000 * scale;
    let handler = rusty::plugin();
    let mut out = Vec::new();
    let heap_before = (probe.heap_used)();
    handler(&rusty::Event { kind: 1, payload: "Dune" }, &mut out);
    let rusty_heap = (probe.heap_used)() as i64 - heap_before as i64;
    out.clear();
    let rusty_us = timed(probe, n, || {
        handler(&rusty::Event { kind: black_box(1), payload: "Dune" }, &mut out);
        out.clear();
    });
    let mut plugin = FlatPlugin::new();
    let plugin_table = plugin.table();
    let mut out = Vec::new();
    let heap_before = (probe.heap_used)();
    flat::deliver(&plugin_table, 1, "Dune", &mut out);
    let flat_heap = (probe.heap_used)() as i64 - heap_before as i64;
    out.clear();
    let flat_us = timed(probe, n, || {
        flat::deliver(black_box(&plugin_table), black_box(1), "Dune", &mut out);
        out.clear();
    });
    lines.push(Line { name: "deliver", iterations: n, rusty_us, flat_us, rusty_heap, flat_heap });

    // The battery widget as a whole (about 150 calls through the table)
    let n = 20 * scale;
    let rusty_us = timed(probe, n, || {
        let mut g = RustyGfx { table, canvas: &mut *ctx.canvas, font, cache: &mut *ctx.cache };
        widget::battery(&mut g, &look, body, black_box(87), true);
    });
    let mut g = FlatGfx { table: flat_table, ctx: ptr };
    let flat_us = timed(probe, n, || {
        widget::battery(&mut g, &look, body, black_box(87), true);
    });
    lines.push(Line { name: "battery_widget", iterations: n, rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });
    lines
}
