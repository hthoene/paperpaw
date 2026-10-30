//! The text calls the table mode tests (`../../native_plugin.rs`): run in
//! the plugin (whose shared functions call the shell's copies through the
//! table) and in the test directly (the functions' own bodies), they must
//! come out the same. Included by both with `#[path]`.

#![allow(
    clippy::unwrap_used,
    reason = "a test's probe: the pack and the canvas are the test's own"
)]

extern crate alloc;

use alloc::vec::Vec;

use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat};
use paperpaw_text::{Font, GlyphCache, TextStyle, draw_text, is_mark, measure, wrap_lines_kerned};

const PACK: &[u8] = include_bytes!("../../../../paperpaw-text/testdata/pack.ppfp.v1");

/// What the calls came to.
#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct TextProbe {
    /// `measure`, a method of `Font` (`&self`), `kern` and `advance`.
    pub width: f32,
    /// `draw_text` (a `&mut` cache, a canvas, a borrowed style): the advance.
    pub advance: f32,
    /// The sum of the canvas's gray levels after it.
    pub gray: u64,
    /// `wrap_lines_kerned` with closures crossing as `dyn`: the lines' ends.
    pub lines: Vec<usize>,
    pub mark: bool,
}

pub fn probe() -> TextProbe {
    let font = Font::from_data(PACK, &[]).unwrap();
    let width = measure(&font, 12.0, "AAAA A");
    let mut frame = Framebuffer::new(64, 32, PixelFormat::Gray8).unwrap();
    let advance;
    {
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        let mut cache = GlyphCache::new(1024);
        let style = TextStyle::new(&font, 0, 12.0, Color::BLACK).bolder(1);
        advance = draw_text(&mut canvas, &mut cache, &style, 4.0, 20, "AA A");
    }
    let view = frame.view();
    let mut gray = 0u64;
    for y in 0..32 {
        for x in 0..64 {
            gray += u64::from(view.pixel(x, y).r);
        }
    }
    let lines = wrap_lines_kerned(
        "AAAA AAA AA A AAAAA",
        |_| 30.0,
        |_, c| if c == ' ' { 3.0 } else { 7.5 },
        |_, _, _| 0.0,
        None,
    )
    .iter()
    .map(|line| line.range.end)
    .collect();
    TextProbe {
        width,
        advance,
        gray,
        lines,
        mark: is_mark('\u{301}'),
    }
}

paperpaw_sdk::service_type!(TextProbe, "hello.text-probe/1");
