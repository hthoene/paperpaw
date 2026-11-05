//! The same work through both paths, timed, and what each call leaves on
//! the heap.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::hint::black_box;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect};
use paperpaw_text::{Font, GlyphCache};

use crate::flat::{self, FLAT, FlatGfx, FlatGfxF, FlatPlugin, HostCtx};
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

/// Which path runs first, so a systematic order effect can be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// All rusty calls, then all flat calls (the first measurement).
    RustyFirst,
    /// All flat calls, then all rusty calls.
    FlatFirst,
    /// Eight alternating chunks, flat first in odd rounds.
    Interleaved,
}

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

/// Times `call(false)` (rusty) and `call(true)` (flat), `n` times each, in `order`.
fn pair(probe: &Probe, n: u32, order: Order, call: &mut dyn FnMut(bool)) -> (u64, u64) {
    match order {
        Order::RustyFirst => {
            let r = timed(probe, n, || call(false));
            let f = timed(probe, n, || call(true));
            (r, f)
        }
        Order::FlatFirst => {
            let f = timed(probe, n, || call(true));
            let r = timed(probe, n, || call(false));
            (r, f)
        }
        Order::Interleaved => {
            let chunk = (n / 8).max(1);
            let (mut r, mut f) = (0, 0);
            for round in 0..8 {
                if round % 2 == 0 {
                    r += timed(probe, chunk, || call(false));
                    f += timed(probe, chunk, || call(true));
                } else {
                    f += timed(probe, chunk, || call(true));
                    r += timed(probe, chunk, || call(false));
                }
            }
            (r, f)
        }
    }
}

fn eff(n: u32, order: Order) -> u32 {
    if order == Order::Interleaved { (n / 8).max(1) * 8 } else { n }
}

/// Runs every pair of calls `scale` times as often as the base counts, the
/// two paths in `order`.
pub fn run(
    probe: &Probe,
    font: &Font,
    cache: &mut GlyphCache,
    canvas: &mut Canvas<'_>,
    scale: u32,
    order: Order,
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
    let mut ctx = HostCtx { canvas, font, cache };
    let ptr: *mut c_void = (&raw mut ctx).cast();

    // (a) a rectangle
    let n = 2000 * scale;
    // SAFETY: the table and its own context.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            (flat_table.fill_rect)(ptr, black_box(10), 10, 30, 12, 0xFF00_0000);
        } else {
            (table.fill_rect)(&mut *(*ptr.cast::<HostCtx>()).canvas, black_box(10), 10, 30, 12, INK);
        }
    });
    lines.push(Line { name: "fill_rect", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (a) a line
    let n = 500 * scale;
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            (flat_table.draw_line)(ptr, black_box(5), 5, 90, 40, 2, 0xFF00_0000);
        } else {
            (table.draw_line)(&mut *(*ptr.cast::<HostCtx>()).canvas, Point::new(black_box(5), 5), Point::new(90, 40), 2, INK);
        }
    });
    lines.push(Line { name: "draw_line", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (b) measuring text
    let n = 300 * scale;
    // SAFETY: as above; the text is valid.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            black_box((flat_table.text_measure)(ptr, 0, black_box(14 * 256), TEXT.as_ptr(), TEXT.len()));
        } else {
            black_box((table.measure)((*ptr.cast::<HostCtx>()).font, black_box(14.0), TEXT));
        }
    });
    lines.push(Line { name: "text_measure", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (b) drawing text
    let n = 60 * scale;
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            black_box((flat_table.text_draw)(ptr, 0, black_box(14 * 256), 0xFF00_0000, 10 * 256, 40, TEXT.as_ptr(), TEXT.len()));
        } else {
            let c = &mut *ptr.cast::<HostCtx>();
            let style = paperpaw_text::TextStyle::new(c.font, 0, black_box(14.0), INK);
            black_box((table.draw_text)(&mut *c.canvas, &mut *c.cache, &style, 10.0, 40, TEXT));
        }
    });
    lines.push(Line { name: "text_draw", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // (c) an event to a plugin, a command back
    let n = 2000 * scale;
    let handler = rusty::plugin();
    let mut out = Vec::new();
    let heap_before = (probe.heap_used)();
    handler(&rusty::Event { kind: 1, payload: "Dune" }, &mut out);
    let rusty_heap = (probe.heap_used)() as i64 - heap_before as i64;
    out.clear();
    let mut plugin = FlatPlugin::new();
    let plugin_table = plugin.table();
    let mut fout = Vec::new();
    let heap_before = (probe.heap_used)();
    flat::deliver(&plugin_table, 1, "Dune", &mut fout);
    let flat_heap = (probe.heap_used)() as i64 - heap_before as i64;
    fout.clear();
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| {
        if flat {
            flat::deliver(black_box(&plugin_table), black_box(1), "Dune", &mut fout);
            fout.clear();
        } else {
            handler(&rusty::Event { kind: black_box(1), payload: "Dune" }, &mut out);
            out.clear();
        }
    });
    lines.push(Line { name: "deliver", iterations: eff(n, order), rusty_us, flat_us, rusty_heap, flat_heap });

    // Where the extra cost of `deliver` comes from: the same call with
    // parts taken away (rusty_us is the Rust-shaped reference).
    let mut variant = |name: &'static str, f: &mut dyn FnMut()| {
        let us = timed(probe, n, || f());
        lines.push(Line { name, iterations: n, rusty_us, flat_us: us, rusty_heap: 0, flat_heap: 0 });
    };
    // the plugin call alone: no host buffer zeroing, no check, no String
    let mut scratch = core::mem::MaybeUninit::<[u8; 96]>::uninit();
    variant("deliver_call_only", &mut || {
        black_box(flat::deliver_raw(black_box(&plugin_table), black_box(1), "Dune", scratch.as_mut_ptr().cast(), 96));
    });
    // call + UTF-8 check, still no String
    variant("deliver_call_utf8", &mut || {
        let n = flat::deliver_raw(black_box(&plugin_table), black_box(1), "Dune", scratch.as_mut_ptr().cast(), 96);
        // SAFETY: the plugin wrote `n` bytes.
        let bytes = unsafe { core::slice::from_raw_parts(scratch.as_ptr().cast::<u8>(), n.max(0) as usize) };
        black_box(core::str::from_utf8(bytes).is_ok());
    });
    // the whole thing without zeroing the 96 byte buffer
    variant("deliver_no_zero", &mut || {
        flat::deliver_uninit(black_box(&plugin_table), black_box(1), "Dune", &mut fout);
        fout.clear();
    });
    // the prepared-buffer path: plugin writes into the host queue's free tail
    let mut queue: Vec<u8> = Vec::with_capacity(512);
    let heap_before = (probe.heap_used)();
    flat::deliver_queued(&plugin_table, 1, "Dune", &mut queue);
    let queued_heap = (probe.heap_used)() as i64 - heap_before as i64;
    queue.clear();
    variant("deliver_queued", &mut || {
        black_box(flat::deliver_queued(black_box(&plugin_table), black_box(1), "Dune", &mut queue));
        queue.clear();
    });
    let _ = queued_heap;


    // Text through the appended C-float entries (no fixed-point conversion)
    let n = 300 * scale;
    // SAFETY: as above; the text is valid.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            let mut out = 0.0f32;
            black_box((flat_table.text_measure_f)(ptr, 0, black_box(14.0), TEXT.as_ptr(), TEXT.len(), &raw mut out));
            black_box(out);
        } else {
            black_box((table.measure)((*ptr.cast::<HostCtx>()).font, black_box(14.0), TEXT));
        }
    });
    lines.push(Line { name: "text_measure_f32", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });
    let n = 60 * scale;
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            let mut out = 0.0f32;
            black_box((flat_table.text_draw_f)(ptr, 0, black_box(14.0), 0xFF00_0000, 10.0, 40, TEXT.as_ptr(), TEXT.len(), &raw mut out));
            black_box(out);
        } else {
            let c = &mut *ptr.cast::<HostCtx>();
            let style = paperpaw_text::TextStyle::new(c.font, 0, black_box(14.0), INK);
            black_box((table.draw_text)(&mut *c.canvas, &mut *c.cache, &style, 10.0, 40, TEXT));
        }
    });
    lines.push(Line { name: "text_draw_f32", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // What the widget uses besides the small calls: round shapes and big text
    let shell = Rect::new(8, 8, 113, 60);
    let n = 200 * scale;
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            (flat_table.fill_round)(ptr, 8, 8, black_box(113), 60, 12, 0xFF00_0000);
        } else {
            (table.fill_round)(&mut *(*ptr.cast::<HostCtx>()).canvas, Rect::new(8, 8, black_box(113), 60), 12, INK);
        }
    });
    lines.push(Line { name: "fill_round", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            (flat_table.stroke_round)(ptr, 8, 8, black_box(113), 60, 5, 12, 0xFF00_0000);
        } else {
            (table.stroke_round)(&mut *(*ptr.cast::<HostCtx>()).canvas, shell, black_box(5), 12, INK);
        }
    });
    lines.push(Line { name: "stroke_round", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });
    let n = 20 * scale;
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| unsafe {
        if flat {
            black_box((flat_table.text_draw)(ptr, 0, black_box(54 * 256), 0xFF00_0000, 10 * 256, 60, b"87".as_ptr(), 2));
        } else {
            let c = &mut *ptr.cast::<HostCtx>();
            let style = paperpaw_text::TextStyle::new(c.font, 0, black_box(54.0), INK);
            black_box((table.draw_text)(&mut *c.canvas, &mut *c.cache, &style, 10.0, 60, "87"));
        }
    });
    lines.push(Line { name: "text_draw_big", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // The same widget without its text calls (the shapes alone)
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| {
        if flat {
            let mut g = FlatGfx { table: flat_table, ctx: ptr };
            widget::battery(&mut widget::NoText(&mut g), &look, body, black_box(87), true);
        } else {
            // SAFETY: as above.
            let c = unsafe { &mut *ptr.cast::<HostCtx>() };
            let mut g = RustyGfx { table, canvas: &mut *c.canvas, font: c.font, cache: &mut *c.cache };
            widget::battery(&mut widget::NoText(&mut g), &look, body, black_box(87), true);
        }
    });
    lines.push(Line { name: "battery_no_text", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });

    // The battery widget as a whole (about 150 calls through the table)
    let n = 20 * scale;
    // SAFETY: as above.
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| {
        if flat {
            let mut g = FlatGfx { table: flat_table, ctx: ptr };
            widget::battery(&mut g, &look, body, black_box(87), true);
        } else {
            // SAFETY: `ptr` is `ctx`, alive for the whole run.
            let c = unsafe { &mut *ptr.cast::<HostCtx>() };
            let mut g = RustyGfx { table, canvas: &mut *c.canvas, font: c.font, cache: &mut *c.cache };
            widget::battery(&mut g, &look, body, black_box(87), true);
        }
    });
    lines.push(Line { name: "battery_widget", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });
    // The widget through the C-float text entries
    let (rusty_us, flat_us) = pair(probe, n, order, &mut |flat| {
        if flat {
            let mut g = FlatGfxF(FlatGfx { table: flat_table, ctx: ptr });
            widget::battery(&mut g, &look, body, black_box(87), true);
        } else {
            // SAFETY: as above.
            let c = unsafe { &mut *ptr.cast::<HostCtx>() };
            let mut g = RustyGfx { table, canvas: &mut *c.canvas, font: c.font, cache: &mut *c.cache };
            widget::battery(&mut g, &look, body, black_box(87), true);
        }
    });
    lines.push(Line { name: "battery_widget_f32", iterations: eff(n, order), rusty_us, flat_us, rusty_heap: 0, flat_heap: 0 });
    lines
}
