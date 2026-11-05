//! #228 prototype (do not merge): runs the flat-boundary bench once at
//! start and logs the times, to measure on the device what a flat
//! `extern "C"` plugin boundary costs next to the Rust-shaped one.

use alloc::vec::Vec;

use log::info;
use paperpaw_flat_proto::bench::{self, Probe};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::PixelFormat;
use paperpaw_text::{Font, GlyphCache};

static PACK: &[u8] = include_bytes!("../../../crates/paperpaw-text/testdata/pack.ppfp.v1");

fn heap_used() -> usize {
    esp_alloc::HEAP.used()
}

/// Runs the bench and logs one line a call kind.
#[inline(never)]
pub fn run() {
    let Ok(font) = Font::from_data(PACK, &[]) else {
        info!("flat-proto: font failed");
        return;
    };
    let Ok(mut frame) = Framebuffer::new(160, 64, PixelFormat::Gray8) else {
        return;
    };
    let mut cache = GlyphCache::new(8 * 1024);
    let mut canvas = frame.canvas();
    let probe = Probe {
        micros: crate::micros,
        heap_used,
    };
    let lines: Vec<bench::Line> = bench::run(&probe, &font, &mut cache, &mut canvas, 1);
    for l in &lines {
        info!(
            "flat-proto: {} n={} rusty_us={} flat_us={} rusty_heap={} flat_heap={}",
            l.name, l.iterations, l.rusty_us, l.flat_us, l.rusty_heap, l.flat_heap
        );
    }
}
