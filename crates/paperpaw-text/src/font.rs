use alloc::borrow::Cow;
use core::fmt;

use crate::anchors::Anchors;
use crate::bitmap::BitmapFont;
#[cfg(feature = "outline")]
use crate::outline::OutlineFont;
use crate::stored::StoredPack;

/// Why a font could not be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontError {
    /// Not a valid font file or font pack.
    Invalid,
    /// The font has no character map.
    NoCharmap,
    /// A font pack of a newer format version.
    UnsupportedVersion,
    /// Scalable fonts need the `outline` feature, which this build lacks.
    OutlinesNotSupported,
    /// A font pack that would take more memory than a pack may on this
    /// device: its characters scattered too widely to keep their index
    /// small.
    TooLarge,
    /// Not enough free memory to open it now; it may open later.
    OutOfMemory,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FontError::Invalid => "not a valid font or font pack",
            FontError::NoCharmap => "font has no character map",
            FontError::UnsupportedVersion => "font pack version not supported",
            FontError::OutlinesNotSupported => "this build only supports bitmap font packs (.ppfp)",
            FontError::TooLarge => "font pack too large for this device's memory",
            FontError::OutOfMemory => "not enough free memory to open the font now",
        })
    }
}

impl core::error::Error for FontError {}

/// Vertical metrics of a font at a size, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LineMetrics {
    /// Distance from the baseline to the top of the line (positive).
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the line (positive).
    pub descent: f32,
    /// Recommended distance between two baselines.
    pub line_height: f32,
}

impl LineMetrics {
    pub const fn new(ascent: f32, descent: f32, line_height: f32) -> Self {
        Self {
            ascent,
            descent,
            line_height,
        }
    }

    /// Rough metrics when a font cannot provide any.
    pub(crate) fn fallback(size: f32) -> Self {
        Self {
            ascent: size * 0.8,
            descent: size * 0.2,
            line_height: size * 1.2,
        }
    }
}

/// A font: a pre-rendered bitmap pack or (with the `outline` feature) a
/// scalable OpenType/TrueType font.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Font {
    Bitmap(BitmapFont),
    /// A pack read a glyph at a time (see [`StoredPack`]).
    Stored(StoredPack),
    #[cfg(feature = "outline")]
    Outline(OutlineFont),
}

impl Font {
    /// Loads a font pack or, if supported, an OpenType/TrueType font.
    /// `variations` applies to variable fonts, e.g. `[("wght", 700.0)]`.
    pub fn from_data(
        data: impl Into<Cow<'static, [u8]>>,
        variations: &[(&str, f32)],
    ) -> Result<Font, FontError> {
        Font::from_cow(data.into(), variations)
    }

    paperpaw_table::shared! {
    /// [`Font::from_data`] without generics, the one of it that is shared
    /// with the firmware.
    pub fn from_cow(
        data: Cow<'static, [u8]>,
        variations: &[(&str, f32)],
    ) -> Result<Font, FontError> {
        if BitmapFont::is_pack(&data) {
            return BitmapFont::new(data).map(Font::Bitmap);
        }
        #[cfg(feature = "outline")]
        {
            OutlineFont::new(data, variations).map(Font::Outline)
        }
        #[cfg(not(feature = "outline"))]
        {
            let _ = variations;
            Err(FontError::OutlinesNotSupported)
        }
    } @ text::FONT_FROM_COW
    }

    paperpaw_table::shared! {
    /// The size `text` is drawn at when asked for `size`: of a pack, the
    /// nearest size that has all its characters, the large ones made for a
    /// few only for a `display` (see [`BitmapFont::size_for`]); else
    /// `size`.
    pub fn size_for(&self, size: f32, text: &str, display: bool) -> f32 {
        match self {
            Font::Bitmap(font) => font.size_for(size, text, display),
            _ => size,
        }
    } @ text::FONT_SIZE_FOR
    }

    paperpaw_table::shared! {
    /// The largest size text can be set in, in pixels: a pack's largest
    /// with every character; `None` for a scalable font, which has every
    /// size.
    pub fn largest_size(&self) -> Option<f32> {
        match self {
            Font::Bitmap(font) => font.largest_size(),
            Font::Stored(font) => font.largest_size(),
            #[cfg(feature = "outline")]
            Font::Outline(_) => None,
        }
    } @ text::FONT_LARGEST_SIZE
    }

    paperpaw_table::shared! {
    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        match self {
            Font::Bitmap(font) => font.line_metrics(size),
            Font::Stored(font) => font.line_metrics(size),
            #[cfg(feature = "outline")]
            Font::Outline(font) => font.line_metrics(size),
        }
    } @ text::FONT_LINE_METRICS
    }

    paperpaw_table::shared! {
    pub fn advance(&self, ch: char, size: f32) -> f32 {
        let ch = self.shown(ch);
        match self {
            Font::Bitmap(font) => font.advance(ch, size),
            Font::Stored(font) => font.advance(ch, size),
            #[cfg(feature = "outline")]
            Font::Outline(font) => font.advance(ch, size),
        }
    } @ text::FONT_ADVANCE
    }

    paperpaw_table::shared! {
    /// How much the pair `left`, `right` moves the second character, in
    /// pixels at `size` (negative: closer): the font's kerning, of the
    /// characters drawn for them ([`Font::shown`]). 0 for a pack read a
    /// glyph at a time ([`StoredPack`]): looking pairs up on the storage
    /// would cost too much time for every character, and keeping them in
    /// memory too much memory on small devices.
    pub fn kern(&self, left: char, right: char, size: f32) -> f32 {
        let (left, right) = (self.shown(left), self.shown(right));
        match self {
            Font::Bitmap(font) => font.kern(left, right, size),
            Font::Stored(_) => 0.0,
            #[cfg(feature = "outline")]
            Font::Outline(font) => font.kern(left, right, size),
        }
    } @ text::FONT_KERN
    }

    /// Where marks sit on `ch` at `size` (see [`crate::anchors`]); for
    /// packs read a glyph at a time and outlines, the
    /// [`GlyphCache`](crate::GlyphCache) keeps them.
    pub(crate) fn anchors(&self, ch: char, size: f32) -> Anchors {
        let ch = self.shown(ch);
        match self {
            Font::Bitmap(font) => font.anchors(ch, size),
            Font::Stored(font) => font.anchors(ch, size),
            #[cfg(feature = "outline")]
            Font::Outline(font) => font.anchors(ch, size),
        }
    }

    /// How many times a pack read from a card could not read an advance
    /// or a glyph (see [`StoredPack::failed_reads`]); 0 for fonts in
    /// memory.
    pub fn failed_reads(&self) -> u32 {
        match self {
            Font::Stored(font) => font.failed_reads(),
            Font::Bitmap(_) => 0,
            #[cfg(feature = "outline")]
            Font::Outline(_) => 0,
        }
    }

    paperpaw_table::shared! {
    pub fn has_char(&self, ch: char) -> bool {
        match self {
            Font::Bitmap(font) => font.has_char(ch),
            Font::Stored(font) => font.has_char(ch),
            #[cfg(feature = "outline")]
            Font::Outline(font) => font.has_char(ch),
        }
    } @ text::FONT_HAS_CHAR
    }

    paperpaw_table::shared! {
    /// The character drawn for `ch`: itself, or where the font lacks it,
    /// a stand-in of nearly the same look it has
    /// ([`charset::stand_in`](crate::charset::stand_in)); else `ch`, for
    /// the font's own replacement. Looked up only for characters that
    /// have a stand-in, so others cost nothing more.
    pub fn shown(&self, ch: char) -> char {
        if crate::charset::stand_in(ch).is_none() {
            return ch;
        }
        let mut shown = ch;
        while let Some(next) = crate::charset::stand_in(shown) {
            if self.has_char(shown) {
                return shown;
            }
            shown = next;
        }
        if self.has_char(shown) { shown } else { ch }
    } @ text::FONT_SHOWN
    }
}
