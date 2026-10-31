//! Text rendering for PaperPaw.
//!
//! - [`Font`]: either a pre-rendered bitmap font pack ([`BitmapFont`], read
//!   in place, works everywhere including the ESP32-C3) or, with the
//!   `outline` feature, a scalable OpenType/TrueType font rendered at
//!   runtime ([`OutlineFont`], via `skrifa`).
//! - A built-in anti-aliasing rasterizer producing 4-bit glyph bitmaps. Packs
//!   are produced with the same rasterizer, so both look identical.
//! - [`GlyphCache`]: a size-bounded LRU cache for runtime-rasterized glyphs.
//! - [`draw_text`], [`measure`] and [`wrap`] for lines and paragraphs.

#![no_std]

// Before the errors of the code the feature adds, whose crates are left
// out there (see Cargo.toml).
#[cfg(all(feature = "outline", target_os = "none"))]
compile_error!(
    "the `outline` feature is for computers, phones and browsers (the emulator's build of a \
     plugin); a device without an operating system, such as the X4 and X4 Pro, draws font packs \
     and loads only plugins built without it: build for it without `outline` \
     (docs/plugins.md, \"Which build fits\")"
);

extern crate alloc;

mod anchors;
// Arabic shaping (#33), used by `draw_text_in`: crate-private until the
// reading path has shown which items plugins need (making them public
// later is an addition). Fuzzing reaches it through `fuzz` (cargo-fuzz
// sets `fuzzing`).
#[allow(dead_code, reason = "parts only the reading path will use (#33)")]
mod arabic;
mod around;
pub mod bidi;
mod bitmap;
mod cache;
pub mod charset;
mod curve;
mod font;
#[cfg(fuzzing)]
pub mod fuzz;
mod glyph;
mod kern_table;
#[cfg(feature = "outline")]
mod kerning;
mod layout;
mod linebreak;
#[cfg(feature = "outline")]
mod marks;
#[cfg(feature = "outline")]
mod outline;
#[cfg_attr(
    not(feature = "outline"),
    allow(
        dead_code,
        reason = "outlines are rastered only with the outline feature"
    )
)]
mod raster;
pub mod sfnt;
mod stored;

pub use around::{Around, draw_text_in, measure_in};
pub use bitmap::{BitmapFont, PackBuilder};
pub use cache::GlyphCache;
pub use curve::ToneCurve;
pub use font::{Font, FontError, LineMetrics};
pub use stored::{PackLimits, PackSource, StoredPack};

/// Whether this build renders scalable (OpenType/TrueType) fonts.
pub const OUTLINES: bool = cfg!(feature = "outline");
pub use glyph::{Glyph, GlyphRef};
pub use layout::{
    Hyphenate, Line, MAX_BREAKS, wrap, wrap_lines, wrap_lines_balanced, wrap_lines_balanced_kerned,
    wrap_lines_balanced_kerned_with_cost, wrap_lines_hyphenated, wrap_lines_kerned,
};
pub use layout::{
    wrap_lines_balanced_in, wrap_lines_balanced_opts, wrap_lines_kerned_in, wrap_lines_kerned_opts,
};
pub use linebreak::UNICODE_VERSION;
#[cfg(feature = "outline")]
pub use outline::OutlineFont;

use paperpaw_graphics::Canvas;
use paperpaw_platform::Color;

paperpaw_table::shared! {
/// Width of `text` in pixels, its pairs kerned ([`Font::kern`]).
pub fn measure(font: &Font, size: f32, text: &str) -> f32 {
    let mut width = 0.0;
    let mut before = None;
    for ch in text.chars() {
        if let Some(left) = before {
            width += font.kern(left, ch, size);
        }
        width += font.advance(ch, size);
        before = Some(ch);
    }
    width
} @ text::TEXT_MEASURE
}

/// How a line of text is drawn: its font (and the font's number in the
/// glyph cache), size and colour, how much bolder, and on what curve.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct TextStyle<'a> {
    pub font: &'a Font,
    /// The font's number in the [`GlyphCache`].
    pub font_index: u16,
    pub size: f32,
    pub color: Color,
    /// Every glyph widened by this many pixels to the right: darker,
    /// bolder text with any font, also bitmap packs. Advances stay as
    /// they are.
    pub bolder: u16,
    /// How the glyphs' coverage is drawn: smooth, on a contrast curve,
    /// or without grays ([`ToneCurve`]).
    pub curve: ToneCurve,
    /// The text goes on from one with a character before it, which
    /// combining marks at its start sit on (see [`glyph_x`]).
    pub based: bool,
}

impl<'a> TextStyle<'a> {
    /// `font` (number `font_index` in the cache) at `size`, in `color`,
    /// as bold as it is.
    pub fn new(font: &'a Font, font_index: u16, size: f32, color: Color) -> Self {
        Self {
            font,
            font_index,
            size,
            color,
            bolder: 0,
            curve: ToneCurve::SMOOTH,
            based: false,
        }
    }

    /// Glyphs widened by `extra` pixels (see [`TextStyle::bolder`]).
    pub fn bolder(mut self, extra: u16) -> Self {
        self.bolder = extra;
        self
    }

    /// Glyphs drawn on `curve` (see [`TextStyle::curve`]).
    pub fn curve(mut self, curve: ToneCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Going on from a text with a character before it (see
    /// [`TextStyle::based`]).
    pub fn based(mut self, based: bool) -> Self {
        self.based = based;
        self
    }
}

paperpaw_table::shared! {
/// Whether `ch` is a combining mark, drawn onto the character before it
/// (an accent, a diaeresis below): the combining blocks of Unicode.
pub fn is_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
} @ text::TEXT_IS_MARK
}

/// Whether `ch` is a control that draws nothing and takes no room: the
/// zero width space (a place to break a line), ZWNJ, ZWJ, the marks,
/// embeddings, overrides and isolates of direction, the word joiner and
/// the invisible operators after it, the byte order mark.
#[must_use]
pub fn is_invisible(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{061C}'
            | '\u{FEFF}'
    )
}

paperpaw_table::shared! {
/// Where a glyph whose bitmap begins `left` pixels from the pen at `pen`
/// is drawn across. A combining mark with no character before it
/// (`lone`) would reach back onto nothing: it sits on a base of no width
/// at the pen instead, never left of where its text begins.
pub fn glyph_x(pen: f32, left: i16, lone: bool) -> i32 {
    let at = raster::floor(pen + 0.5);
    let x = at.saturating_add(i32::from(left));
    if lone { x.max(at) } else { x }
} @ text::TEXT_GLYPH_X
}

impl core::fmt::Debug for TextStyle<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextStyle")
            .field("font_index", &self.font_index)
            .field("size", &self.size)
            .field("color", &self.color)
            .field("bolder", &self.bolder)
            .field("curve", &self.curve)
            .finish_non_exhaustive()
    }
}

paperpaw_table::shared! {
/// Draws one line of `text` in `style` with its baseline at `baseline` and
/// returns the advance in pixels, its pairs kerned as [`measure`] kerns
/// them. Newlines are not interpreted; use [`wrap`] first.
pub fn draw_text(
    canvas: &mut Canvas<'_>,
    cache: &mut GlyphCache,
    style: &TextStyle<'_>,
    x: f32,
    baseline: i32,
    text: &str,
) -> f32 {
    let TextStyle {
        font,
        font_index,
        size,
        mut based,
        ..
    } = *style;
    let mut pen = x;
    let mut before = None;
    for ch in text.chars() {
        if let Some(left) = before {
            pen += font.kern(left, ch, size);
        }
        before = Some(ch);
        let glyph = cache.glyph(font_index, font, ch, size);
        let mark = is_mark(ch);
        // Saturating: a pen far out (absurd advances) draws nothing.
        let gx = glyph_x(pen, glyph.left, mark && !based);
        based |= !mark;
        paint(canvas, &glyph, gx, baseline, style);
        pen += glyph.advance;
    }
    pen - x
} @ text::TEXT_DRAW_TEXT
}

/// Draws `glyph` with its pen at `gx` across and `baseline`, in `style`'s
/// colour, weight and curve.
fn paint(
    canvas: &mut Canvas<'_>,
    glyph: &GlyphRef<'_>,
    gx: i32,
    baseline: i32,
    style: &TextStyle<'_>,
) {
    let TextStyle {
        color,
        bolder: extra,
        curve: tone,
        ..
    } = *style;
    let gy = baseline.saturating_add(i32::from(glyph.top));
    if !glyph.is_empty() && extra == 0 {
        // The usual weight: each pixel's own coverage, on its curve.
        let level = glyph.levels();
        canvas.draw_mask(
            gx,
            gy,
            u32::from(glyph.width),
            u32::from(glyph.height),
            color,
            |px, py| tone.alpha(level(px, py)),
        );
    } else if !glyph.is_empty() {
        let width = u32::from(glyph.width);
        canvas.draw_mask(
            gx,
            gy,
            width + u32::from(extra),
            u32::from(glyph.height),
            color,
            |px, py| {
                // The strongest coverage of the pixels up to `extra`
                // to the left, on its curve (curves rise, so the
                // strongest stays the strongest).
                let level = (0..=u32::from(extra))
                    .filter(|k| *k <= px && px - k < width)
                    .map(|k| glyph.level((px - k) as u16, py as u16))
                    .max()
                    .unwrap_or(0);
                tone.alpha(level)
            },
        );
    }
}

/// Renders `chars` of `font` at each of `sizes` into a bitmap font pack,
/// with the font's kerning of them and, for Arabic and Hebrew, where its
/// marks sit on its letters (an anchors table, see
/// [`BitmapFont::ANCHORS`]). Characters the font lacks are skipped.
#[cfg(feature = "outline")]
pub fn build_pack(
    font: &OutlineFont,
    sizes: &[f32],
    chars: impl IntoIterator<Item = char> + Clone,
) -> alloc::vec::Vec<u8> {
    let mut builder = PackBuilder::new();
    let kerned: alloc::vec::Vec<char> = chars.clone().into_iter().collect();
    builder.kerning(&font.kerning_pairs(&kerned));
    // Every key of a strike, in order, so that a key's anchors are where
    // its glyph is.
    let mut keys: alloc::vec::Vec<char> = kerned
        .iter()
        .copied()
        .filter(|&ch| font.has_char(ch))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let anchored: alloc::vec::Vec<_> = sizes
        .iter()
        .map(|&size| font.anchors_of(&keys, size))
        .collect();
    if anchored.iter().flatten().any(|(_, a)| !a.is_empty()) {
        builder.table(BitmapFont::ANCHORS, 1, &anchors::build(&anchored));
    }
    for &size in sizes {
        let glyphs = chars
            .clone()
            .into_iter()
            .filter(|&ch| font.has_char(ch))
            .map(|ch| (ch, font.rasterize(ch, size)))
            .collect();
        builder.add_strike(size, font.line_metrics(size), glyphs);
    }
    builder.build()
}

#[cfg(all(test, feature = "outline"))]
mod tests {
    use super::*;

    const FONT: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/SourceSans3.ttf");

    #[test]
    fn packs_render_exactly_like_runtime_rasterization() {
        let outline = OutlineFont::new(FONT, &[("wght", 400.0)]).unwrap();
        let pack = BitmapFont::new(build_pack(&outline, &[24.0], "Agü€?".chars())).unwrap();
        // Pixels are identical; advances are stored in 1/64 px.
        let same = |packed: GlyphRef<'_>, runtime: GlyphRef<'_>| {
            let advance = raster::abs(packed.advance - runtime.advance) <= 1.0 / 128.0;
            let pixels = GlyphRef {
                advance: runtime.advance,
                ..packed
            } == runtime;
            advance && pixels
        };
        for ch in "Agü€".chars() {
            let runtime = outline.rasterize(ch, 24.0);
            assert!(same(pack.glyph(ch, 24.0), runtime.as_ref()), "{ch}");
        }
        // Not in the pack: falls back to '?'.
        let question = outline.rasterize('?', 24.0);
        assert!(same(pack.glyph('Ж', 24.0), question.as_ref()));
    }

    #[test]
    fn rasterizes_visible_glyphs() {
        // The font's default instance is its lightest; regular has stems.
        let font = OutlineFont::new(FONT, &[("wght", 400.0)]).unwrap();
        let glyph = font.rasterize('H', 32.0);
        let r = glyph.as_ref();
        assert!(r.width > 10 && r.height > 15, "{r:?}");
        assert!(r.top < 0, "glyph should sit above the baseline");
        let full = (0..r.height)
            .flat_map(|y| (0..r.width).map(move |x| (x, y)))
            .filter(|&(x, y)| r.coverage(x, y) == 255)
            .count();
        assert!(full > 20, "stems should be fully covered");
        assert!(font.rasterize(' ', 32.0).as_ref().is_empty());
        assert!(font.line_metrics(32.0).line_height > 32.0);
    }
}
