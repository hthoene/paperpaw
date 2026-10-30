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
use paperpaw_text::{
    Around, Font, GlyphCache, TextStyle, draw_text, draw_text_in, is_mark, measure, measure_in,
    wrap_lines_kerned,
};

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
    /// `measure_in` and `draw_text_in` of a text that needs the bidi code
    /// (right-to-left letters, brackets): its width and ink.
    pub width_in: f32,
    pub gray_in: u64,
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
    let around = Around::NONE;
    let rtl = "A (\u{05d0}\u{05d1}) B";
    let width_in = measure_in(&font, 12.0, rtl, &around);
    let mut again = Framebuffer::new(64, 32, PixelFormat::Gray8).unwrap();
    {
        let mut canvas = again.canvas();
        canvas.clear(Color::WHITE);
        let mut cache = GlyphCache::new(1024);
        let style = TextStyle::new(&font, 0, 12.0, Color::BLACK);
        draw_text_in(&mut canvas, &mut cache, &style, 2.0, 20, rtl, &around);
    }
    let view = again.view();
    let mut gray_in = 0u64;
    for y in 0..32 {
        for x in 0..64 {
            gray_in += u64::from(view.pixel(x, y).r);
        }
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
        width_in,
        gray_in,
    }
}

paperpaw_sdk::service_type!(TextProbe, "hello.text-probe/1");
