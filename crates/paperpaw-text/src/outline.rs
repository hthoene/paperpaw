//! Runtime rendering of OpenType/TrueType fonts via `skrifa`.

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

use skrifa::instance::{Location, Size};
use skrifa::outline::{DrawSettings, HintingInstance, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::Hinting;
use crate::anchors::Anchors;
use crate::font::{FontError, LineMetrics};
use crate::glyph::{self, Glyph, pack_coverage};
use crate::kerning::Kerning;
use crate::marks::Marks;
use crate::raster::{self, Path, Pt};

/// A scalable font, optionally at a fixed position of its variation axes
/// (e.g. weight 700 of a variable font).
///
/// The font data is borrowed for built-in fonts or owned for fonts loaded at
/// runtime.
#[derive(Clone)]
pub struct OutlineFont {
    data: Cow<'static, [u8]>,
    location: Location,
}

impl fmt::Debug for OutlineFont {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutlineFont")
            .field("bytes", &self.data.len())
            .finish_non_exhaustive()
    }
}

impl OutlineFont {
    /// Loads a font. `variations` sets axes such as `[("wght", 700.0)]`;
    /// unknown axes are ignored.
    pub fn new(
        data: impl Into<Cow<'static, [u8]>>,
        variations: &[(&str, f32)],
    ) -> Result<OutlineFont, FontError> {
        let data = data.into();
        let font = FontRef::new(&data).map_err(|_| FontError::Invalid)?;
        // Any mapping will do: icon fonts have no letters.
        if font.charmap().mappings().next().is_none() {
            return Err(FontError::NoCharmap);
        }
        let location = font.axes().location(variations.iter().copied());
        Ok(OutlineFont { data, location })
    }

    fn font_ref(&self) -> Option<FontRef<'_>> {
        // Validated in `new`; `None` is unreachable in practice.
        FontRef::new(&self.data).ok()
    }

    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        let Some(font) = self.font_ref() else {
            return LineMetrics::fallback(size);
        };
        let metrics = font.metrics(Size::new(size), &self.location);
        let descent = -metrics.descent;
        LineMetrics {
            ascent: metrics.ascent,
            descent,
            line_height: metrics.ascent + descent + metrics.leading,
        }
    }

    pub fn has_char(&self, ch: char) -> bool {
        self.font_ref()
            .is_some_and(|font| font.charmap().map(ch).is_some())
    }

    /// The glyph of `ch`; for a key of a glyph without code point (an
    /// Indic conjunct a pack is made with, `crate::indic::GLYPHS`), that
    /// glyph, if the font has it.
    fn glyph_id(font: &FontRef<'_>, ch: char) -> GlyphId {
        use skrifa::raw::TableProvider as _;
        let charmap = font.charmap();
        let count = font.maxp().map_or(0, |m| m.num_glyphs());
        let keyed = crate::indic::glyph_of_key(ch)
            .filter(|&n| n < count)
            .map(|n| GlyphId::new(u32::from(n)));
        charmap
            .map(ch)
            .or(keyed)
            .or_else(|| charmap.map('\u{fffd}'))
            .unwrap_or(GlyphId::NOTDEF)
    }

    pub fn advance(&self, ch: char, size: f32) -> f32 {
        let Some(font) = self.font_ref() else {
            return 0.0;
        };
        font.glyph_metrics(Size::new(size), &self.location)
            .advance_width(Self::glyph_id(&font, ch))
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// How much the pair `left`, `right` moves the second character, in
    /// pixels at `size` (negative: closer): the font's kerning
    /// ([`OutlineFont::kerning_pairs`]). 0 where either character is
    /// missing.
    pub fn kern(&self, left: char, right: char, size: f32) -> f32 {
        let Some(font) = self.font_ref() else {
            return 0.0;
        };
        let charmap = font.charmap();
        let (Some(l), Some(r)) = (charmap.map(left), charmap.map(right)) else {
            return 0.0;
        };
        let units = Kerning::new(font.clone(), self.location.coords()).pair((left, right), l, r);
        if units == 0.0 {
            return 0.0;
        }
        units * size / units_per_em(&font)
    }

    /// The kerning pairs among `chars`, each with how much it moves the
    /// second character, in ems (negative: closer): the pairs of the
    /// font's GPOS `kern` feature, or of its legacy `kern` table where it
    /// has no GPOS kerning; at the instance of a variable font. Pairs that
    /// change nothing and characters the font lacks are left out.
    pub fn kerning_pairs(&self, chars: &[char]) -> Vec<(char, char, f32)> {
        let Some(font) = self.font_ref() else {
            return Vec::new();
        };
        let charmap = font.charmap();
        let glyphs: Vec<(char, GlyphId)> = chars
            .iter()
            .filter_map(|&ch| Some((ch, charmap.map(ch)?)))
            .collect();
        let kerning = Kerning::new(font.clone(), self.location.coords());
        if kerning.is_empty() {
            return Vec::new();
        }
        let em = units_per_em(&font);
        let mut pairs = Vec::new();
        for &(left, l) in &glyphs {
            for &(right, r) in &glyphs {
                let units = kerning.pair((left, right), l, r);
                if units != 0.0 {
                    pairs.push((left, right, units / em));
                }
            }
        }
        pairs
    }

    /// Where marks sit on `ch` at `size` (see [`crate::anchors`]), from
    /// the font's GPOS: Arabic and Hebrew alone.
    pub(crate) fn anchors(&self, ch: char, size: f32) -> Anchors {
        let Some(font) = self.font_ref() else {
            return Anchors::NONE;
        };
        Marks::new(font, self.location.coords()).anchors(ch, size)
    }

    /// The anchors of each of `chars` at `size`, read with the font's
    /// lookups found once: for packs.
    pub(crate) fn anchors_of(&self, chars: &[char], size: f32) -> Vec<(char, Anchors)> {
        let Some(font) = self.font_ref() else {
            return Vec::new();
        };
        let marks = Marks::new(font, self.location.coords());
        chars
            .iter()
            .map(|&ch| (ch, marks.anchors(ch, size)))
            .collect()
    }

    /// The substitutions of the Bengali and Devanagari among `chars`, as a
    /// pack keeps them (see `crate::indic::pack`), and the keys of the
    /// glyphs without code point they draw.
    pub(crate) fn indic_substitutions(&self, chars: &[char]) -> Option<crate::indic::pack::Made> {
        crate::indic::pack::make(self.font_ref()?, chars)
    }

    /// Rasterizes `ch` as the outline is drawn (unhinted); missing
    /// characters render as U+FFFD or `.notdef`.
    pub fn rasterize(&self, ch: char, size: f32) -> Glyph {
        self.rasterize_chars([ch], size, Hinting::None)
            .pop()
            .map_or_else(|| Glyph::empty(0.0), |(_, glyph)| glyph)
    }

    /// Rasterizes each of `chars` at `size` as [`OutlineFont::rasterize`]
    /// does, its ink fitted to the pixel grid as `hinting` says (advances
    /// and metrics as they are), the hinter set up once for all of them:
    /// for packs. How a strike is fitted is the job's, not the font's: the
    /// hinter is made for one size, and the font itself (which plugins
    /// hand the kernel as [`Font::Outline`](crate::Font)) stays as it is.
    pub fn rasterize_chars(
        &self,
        chars: impl IntoIterator<Item = char>,
        size: f32,
        hinting: Hinting,
    ) -> Vec<(char, Glyph)> {
        let Some(font) = self.font_ref() else {
            return chars
                .into_iter()
                .map(|ch| (ch, Glyph::empty(0.0)))
                .collect();
        };
        let outlines = font.outline_glyphs();
        // A hinter that cannot be made for this font or size draws the
        // outline as it is.
        let hinter = hinting.options().and_then(|options| {
            HintingInstance::new(&outlines, Size::new(size), &self.location, options).ok()
        });
        chars
            .into_iter()
            .map(|ch| {
                let glyph = self.rasterize_with(&font, &outlines, hinter.as_ref(), ch, size);
                (ch, glyph)
            })
            .collect()
    }

    fn rasterize_with(
        &self,
        font: &FontRef<'_>,
        outlines: &skrifa::outline::OutlineGlyphCollection<'_>,
        hinter: Option<&HintingInstance>,
        ch: char,
        size: f32,
    ) -> Glyph {
        let id = Self::glyph_id(font, ch);
        let advance = font
            .glyph_metrics(Size::new(size), &self.location)
            .advance_width(id)
            .unwrap_or(0.0)
            .max(0.0);
        let mut pen = Pen::default();
        if let Some(outline) = outlines.get(id) {
            let settings = match hinter {
                Some(hinter) => DrawSettings::hinted(hinter, false),
                None => DrawSettings::unhinted(Size::new(size), &self.location),
            };
            // A broken glyph renders as empty instead of failing the page.
            let _ = outline.draw(settings, &mut pen);
        }
        pen.0.close();

        let Some((x0, y0, x1, y1)) = pen.0.bounds() else {
            return Glyph::empty(advance);
        };
        // Points far out (or an em of a few units) would ask for a bitmap
        // of gigabytes: such a glyph renders as empty.
        let reach = glyph::reach(size);
        if [x0, y0, x1, y1]
            .iter()
            .any(|c| !(-reach..=reach).contains(c))
        {
            return Glyph::empty(advance);
        }
        let (width, height) = ((x1 - x0) as usize, (y1 - y0) as usize);
        if !glyph::fits(width, height, size) {
            return Glyph::empty(advance);
        }
        // Shifted in place rather than copied: outlines can be long.
        let mut lines = pen.0.lines;
        for p in lines.iter_mut().flat_map(|(a, b)| [a, b]) {
            p.x -= x0 as f32;
            p.y -= y0 as f32;
        }
        let Some(data) = raster::rasterize(&lines, width, height)
            .and_then(|coverage| pack_coverage(&coverage, width, height))
        else {
            return Glyph::empty(advance);
        };
        // Within `reach`: offsets fit an `i16`, sides a `u16`.
        Glyph {
            format: crate::glyph::COVERAGE4,
            left: x0 as i16,
            top: y0 as i16,
            width: width as u16,
            height: height as u16,
            advance,
            data,
        }
    }
}

/// The font's units per em; 1000 where it does not say.
fn units_per_em(font: &FontRef<'_>) -> f32 {
    use skrifa::raw::TableProvider;
    match font.head().map(|head| head.units_per_em()) {
        Ok(units) if units > 0 => f32::from(units),
        _ => 1000.0,
    }
}

/// Receives outlines from skrifa (y up) and stores them y-down.
#[derive(Default)]
struct Pen(Path);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(Pt { x, y: -y });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(Pt { x, y: -y });
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(Pt { x: cx0, y: -cy0 }, Pt { x, y: -y });
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.cubic_to(
            Pt { x: cx0, y: -cy0 },
            Pt { x: cx1, y: -cy1 },
            Pt { x, y: -y },
        );
    }

    fn close(&mut self) {
        self.0.close();
    }
}
