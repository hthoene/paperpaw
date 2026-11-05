//! Shared test workload: matches #228 attachments 699/700 without SD access.

extern crate alloc;

use alloc::{format, rc::Rc, string::String, vec::Vec};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat, Storage};
use paperpaw_sdk::{Font, FontSpec, Fonts, Id, MemoryStorage};
use paperpaw_text::{Glyph, LineMetrics, PackBuilder, TextInk};

#[allow(
    dead_code,
    reason = "only slot14's backend is exercised by this harness"
)]
#[path = "../../../../crates/paperpaw-core/src/abi7/graphics.rs"]
mod graphics;

/// Exactly 4096 UTF-8 bytes, with deterministic font routing.
pub fn fixture(case: &str) -> Result<String, &'static str> {
    match case {
        "hebrew" => Ok("א".repeat(2048)),
        "rlm-ascii" => Ok(format!("\u{200f}{}", "a".repeat(4093))),
        "rlm-alternating" => Ok(format!("\u{200f}{}a", "a.".repeat(2046))),
        "ascii" | "bitmap-ascii" => Ok("a".repeat(4096)),
        "bitmap-A" => Ok("A".repeat(4096)),
        _ => Err("case"),
    }
}

fn pack(chars: &str) -> Vec<u8> {
    let mut chars: Vec<_> = chars.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    let mut pack = PackBuilder::new();
    pack.add_strike(
        12.0,
        LineMetrics::new(9.0, 3.0, 14.0),
        chars
            .into_iter()
            .map(|c| {
                (
                    c,
                    Glyph::empty(if paperpaw_text::is_mark(c) { 0.0 } else { 5.0 }),
                )
            })
            .collect(),
    );
    pack.build()
}

/// Fresh registry and MemoryStorage for every measured invocation. The fixture
/// pack has real bitmap data; synthetic packs have empty glyphs by design.
pub fn fonts(case: &str) -> Result<Fonts, &'static str> {
    let mut fonts = Fonts::new(4096);
    let chars = match case {
        "rlm-alternating" => ". ?",
        "bitmap-ascii" | "bitmap-A" => "א",
        "ascii" | "hebrew" | "rlm-ascii" => {
            "a.fil ?()0123456789אבג\u{5b8}بلاإ\u{64e}\u{fe8e}\u{fe90}\u{fe91}\u{fe92}\u{fedf}\u{fee0}\u{fefb}\u{fefc}한가\u{1112}\u{1161}\u{11ab}क\u{93f}😀\u{301}"
        }
        _ => return Err("case"),
    };
    let font = Font::from_data(pack(chars), &[]).map_err(|_| "font")?;
    fonts
        .add(FontSpec::new(Id::new_static("probe.font"), font))
        .map_err(|_| "font-add")?;
    if matches!(case, "rlm-alternating" | "bitmap-ascii" | "bitmap-A") {
        let data = if matches!(case, "bitmap-ascii" | "bitmap-A") {
            include_bytes!("../../../../crates/paperpaw-text/testdata/pack.ppfp.v1").to_vec()
        } else {
            pack(
                "אבג\u{5b8}بلاإ\u{64e}\u{fe8e}\u{fe90}\u{fe91}\u{fe92}\u{fedf}\u{fee0}\u{fefb}\u{fefc}한가\u{1112}\u{1161}\u{11ab}a",
            )
        };
        let storage: Rc<dyn Storage> =
            Rc::new(MemoryStorage::new().with("plugins/probe/letters.ppfp", data));
        fonts
            .add_extension(
                Id::new_static("probe.letters"),
                "plugins/probe/letters.ppfp".into(),
                Id::new_static("probe.font"),
            )
            .map_err(|_| "extension")?;
        if !fonts.load(&storage).is_empty() {
            return Err("font-load");
        }
    }
    Ok(fonts)
}

pub fn frame(mode: &str) -> Result<Framebuffer, &'static str> {
    match mode {
        "blind" => Ok(Framebuffer::blind(40, 32)),
        "raster" | "describing" | "rtl-ink" => {
            Framebuffer::new(40, 32, PixelFormat::Gray8).map_err(|_| "frame")
        }
        _ => Err("mode"),
    }
}

/// Resolve handles and any setup allocation before the measured call.
pub fn handle<'a>(
    fonts: &'a Fonts,
    mode: &str,
) -> Result<paperpaw_sdk::FontHandle<'a>, &'static str> {
    let mut font = fonts.get("probe.font").ok_or("font-handle")?.ligatures();
    if mode == "rtl-ink" {
        font = font.inked(TextInk::DEFAULT.with_flags(TextInk::RIGHT_TO_LEFT));
    }
    Ok(font)
}

/// One direct draw or the unchanged slot14 safe backend. This bypasses raw
/// pointer/handle/context admission and does not qualify the production ABI.
pub fn draw_handle(
    font: &paperpaw_sdk::FontHandle<'_>,
    frame: &mut Framebuffer,
    text: &str,
    adapter: bool,
) -> Result<f32, &'static str> {
    if adapter {
        graphics::text_draw(
            &mut frame.canvas(),
            font,
            12.0,
            0.125,
            16,
            text,
            Color::WHITE,
        )
        .map_err(|_| "draw")
    } else {
        let width = font.draw(&mut frame.canvas(), 12.0, 0.125, 16, text, Color::WHITE);
        if width.is_finite() && width >= 0.0 {
            Ok(width)
        } else {
            Err("advance")
        }
    }
}

#[cfg(test)]
pub fn draw(
    fonts: &Fonts,
    frame: &mut Framebuffer,
    text: &str,
    mode: &str,
    adapter: bool,
) -> Result<f32, &'static str> {
    draw_handle(&handle(fonts, mode)?, frame, text, adapter)
}
