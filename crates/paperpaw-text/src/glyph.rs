use alloc::vec::Vec;

/// A glyph's bitmap format: 4-bit coverage, two pixels per byte, high
/// nibble first, rows padded to whole bytes. The only one so far; a later
/// build may add others (8-bit coverage, colour), which a build that
/// doesn't know them reads as empty.
pub(crate) const COVERAGE4: u8 = 0;

/// The same coverage stored compactly (`crate::packed`, the strike flag of a
/// pack), read row by row: [`GlyphRef::levels`] and [`GlyphRef::level`] give
/// the pixels either way.
pub(crate) const PACKED4: u8 = 1;

/// A borrowed glyph bitmap with 4-bit coverage.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct GlyphRef<'a> {
    /// Offset of the bitmap's left edge from the pen position.
    pub left: i16,
    /// Offset of the bitmap's top edge from the baseline (negative = above).
    pub top: i16,
    pub width: u16,
    pub height: u16,
    /// Horizontal advance in pixels.
    pub advance: f32,
    /// Two pixels per byte, high nibble first, rows padded to whole bytes.
    pub(crate) data: &'a [u8],
    /// The format of `data` ([`COVERAGE4`]).
    pub(crate) format: u8,
}

impl GlyphRef<'_> {
    /// Coverage of a pixel, 0 (empty) to 255 (full). Out-of-range reads and
    /// truncated data read as empty.
    pub fn coverage(&self, x: u16, y: u16) -> u8 {
        self.level(x, y) * 17
    }

    /// Coverage of a pixel as stored, 0 (empty) to 15 (full); read as
    /// [`GlyphRef::coverage`] reads it.
    #[inline]
    pub(crate) fn level(&self, x: u16, y: u16) -> u8 {
        if self.format == PACKED4 {
            // Decoded from the first row to `y`: a measure over a whole glyph
            // uses `levels`.
            return crate::packed::Rows::new(self.data, self.width, self.height)
                .level(u32::from(x), u32::from(y));
        }
        let stride = usize::from(self.width).div_ceil(2);
        if self.format != COVERAGE4 {
            return 0;
        }
        let Some(&byte) = self.data.get(usize::from(y) * stride + usize::from(x) / 2) else {
            return 0;
        };
        if x.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0f
        }
    }

    /// The level (0 to 15) of every pixel, the length of a row worked out
    /// once rather than for each pixel. A packed glyph is decoded a row at a
    /// time (rows asked in order cost one pass; an earlier row starts over).
    #[inline]
    pub fn levels(&self) -> impl Fn(u32, u32) -> u8 + '_ {
        let stride = usize::from(self.width).div_ceil(2);
        let source = match self.format {
            COVERAGE4 => Levels::Raw(self.data),
            PACKED4 => Levels::Packed(crate::packed::Rows::new(self.data, self.width, self.height)),
            _ => Levels::Raw(&[]),
        };
        move |x, y| match &source {
            Levels::Packed(rows) => rows.level(x, y),
            Levels::Raw(data) => {
                let at = (y as usize) * stride + (x as usize) / 2;
                match data.get(at) {
                    Some(&byte) if x % 2 == 0 => byte >> 4,
                    Some(&byte) => byte & 0x0f,
                    None => 0,
                }
            }
        }
    }

    /// The first and last row, the first and last column (in this order) of
    /// the pixels at least half covered, as panels of black and white show
    /// them; `None` for a glyph without ink. One pass in order, which a
    /// packed glyph reads cheaply (a column at a time would decode it again and
    /// again).
    pub fn ink_box(&self) -> Option<(u16, u16, u16, u16)> {
        let level = self.levels();
        let (mut top, mut bottom, mut left, mut right) = (u16::MAX, 0, u16::MAX, 0);
        let mut any = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if level(u32::from(x), u32::from(y)) >= 8 {
                    any = true;
                    top = top.min(y);
                    bottom = bottom.max(y);
                    left = left.min(x);
                    right = right.max(x);
                }
            }
        }
        any.then_some((top, bottom, left, right))
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A glyph's rows as the opacities a tone curve gives their levels, a run of
/// a row at a time (`Canvas::draw_mask_rows`): what a draw asks for pixel by
/// pixel with [`GlyphRef::levels`] costs the choice of the storage and the
/// row's start again for each.
pub(crate) struct AlphaRows<'a> {
    tone: [u8; 16],
    stride: usize,
    source: Levels<'a>,
}

impl AlphaRows<'_> {
    /// Fills `out` with the opacities of the pixels `dx`, `dx + 1`... of row
    /// `dy` (0 beyond the glyph's data).
    #[inline]
    pub(crate) fn fill(&self, dy: u32, dx: u32, out: &mut [u8]) {
        let x = dx as usize;
        match &self.source {
            Levels::Raw(data) => {
                let row = data.get(dy as usize * self.stride..).unwrap_or(&[]);
                let mut at = 0;
                let mut x = x;
                // From the second pixel of a byte, then two pixels a byte.
                if x % 2 == 1 && !out.is_empty() {
                    let byte = row.get(x / 2).copied().unwrap_or(0);
                    out[0] = self.tone[usize::from(byte & 0x0f)];
                    (at, x) = (1, x + 1);
                }
                let bytes = row.get(x / 2..).unwrap_or(&[]);
                let (pairs, _) = out[at..].as_chunks_mut::<2>();
                let mut used = 0;
                for (pair, &byte) in pairs.iter_mut().zip(bytes) {
                    pair[0] = self.tone[usize::from(byte >> 4)];
                    pair[1] = self.tone[usize::from(byte & 0x0f)];
                    used += 1;
                }
                // Past the data: empty (the buffer holds the run before).
                let done = at + used * 2;
                for alpha in &mut out[done..] {
                    *alpha = 0;
                }
                if let (true, Some(&byte)) = (done < out.len(), bytes.get(used)) {
                    // A last pixel alone: the high half of its byte.
                    out[done] = self.tone[usize::from(byte >> 4)];
                }
            }
            Levels::Packed(rows) => {
                let row = rows.row(dy);
                for (n, alpha) in out.iter_mut().enumerate() {
                    let level = row.get(x + n).map_or(0, core::cell::Cell::get);
                    *alpha = self.tone[usize::from(level & 0x0f)];
                }
            }
        }
    }
}

impl GlyphRef<'_> {
    /// The rows of this glyph in the opacities of `tone` (its 16 alphas).
    pub(crate) fn alpha_rows(&self, tone: [u8; 16]) -> AlphaRows<'_> {
        let source = match self.format {
            COVERAGE4 => Levels::Raw(self.data),
            PACKED4 => Levels::Packed(crate::packed::Rows::new(self.data, self.width, self.height)),
            _ => Levels::Raw(&[]),
        };
        AlphaRows {
            tone,
            stride: usize::from(self.width).div_ceil(2),
            source,
        }
    }
}

/// Where [`GlyphRef::levels`] reads from.
#[allow(
    clippy::large_enum_variant,
    reason = "on the stack of one glyph draw, never boxed: no heap on the draw path"
)]
enum Levels<'a> {
    Raw(&'a [u8]),
    Packed(crate::packed::Rows<'a>),
}

/// An owned glyph bitmap (e.g. rasterized at runtime and cached).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Glyph {
    pub left: i16,
    pub top: i16,
    pub width: u16,
    pub height: u16,
    pub advance: f32,
    pub(crate) data: Vec<u8>,
    /// The format of `data` ([`COVERAGE4`]).
    pub(crate) format: u8,
}

impl Glyph {
    /// An empty glyph that only advances the pen (e.g. a space).
    pub fn empty(advance: f32) -> Self {
        Self {
            format: crate::glyph::COVERAGE4,
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            advance,
            data: Vec::new(),
        }
    }

    pub fn as_ref(&self) -> GlyphRef<'_> {
        GlyphRef {
            format: self.format,
            left: self.left,
            top: self.top,
            width: self.width,
            height: self.height,
            advance: self.advance,
            data: &self.data,
        }
    }

    /// Approximate heap usage, for cache accounting: the bitmap's memory,
    /// which a glyph read into the memory of another may not fill.
    pub fn byte_size(&self) -> usize {
        self.data.capacity() + core::mem::size_of::<Glyph>()
    }
}

/// Largest glyph bitmap in pixels (2048 x 2048); rasterizing one takes
/// four bytes a pixel.
pub(crate) const MAX_PIXELS: usize = 1 << 22;

/// How far a glyph at `size` pixels may reach from the pen position, in
/// pixels: four ems, at least 16 pixels, at most what a glyph's offsets
/// hold. Outlines reaching further (a broken or hostile font) render as
/// empty, so packs made with the same rasterizer hold no larger glyphs.
pub(crate) fn reach(size: f32) -> i32 {
    (size * 4.0).clamp(16.0, f32::from(i16::MAX / 2)) as i32
}

/// Whether a bitmap of `width` x `height` pixels is one a glyph at `size`
/// pixels may have (see [`reach`] and [`MAX_PIXELS`]).
pub(crate) fn fits(width: usize, height: usize, size: f32) -> bool {
    let side = 2 * reach(size) as usize;
    width <= side && height <= side && width * height <= MAX_PIXELS
}

/// `len` zeroed items; `None` if memory is short.
pub(crate) fn zeroed<T: Clone + Default>(len: usize) -> Option<Vec<T>> {
    let mut items = Vec::new();
    items.try_reserve_exact(len).ok()?;
    items.resize(len, T::default());
    Some(items)
}

/// Packs 8-bit coverage into the 4-bit glyph format; `None` if memory is
/// short.
#[cfg_attr(
    not(feature = "outline"),
    allow(
        dead_code,
        reason = "only outlines drawn with the outline feature are packed"
    )
)]
pub(crate) fn pack_coverage(coverage: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let stride = width.div_ceil(2);
    let mut data = zeroed(stride.checked_mul(height)?)?;
    for y in 0..height {
        for x in 0..width {
            let nibble = ((u16::from(coverage[y * width + x]) + 8) / 17) as u8;
            let shift = if x % 2 == 0 { 4 } else { 0 };
            data[y * stride + x / 2] |= nibble << shift;
        }
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_round_trips_through_packing() {
        let coverage = [0, 255, 136, 17, 255];
        let glyph = Glyph {
            format: crate::glyph::COVERAGE4,
            left: 0,
            top: 0,
            width: 5,
            height: 1,
            advance: 5.0,
            data: pack_coverage(&coverage, 5, 1).unwrap(),
        };
        let r = glyph.as_ref();
        let read: Vec<u8> = (0..5).map(|x| r.coverage(x, 0)).collect();
        assert_eq!(read, coverage);
        assert_eq!(r.coverage(9, 9), 0);
    }

    #[test]
    fn levels_read_as_each_level() {
        // Odd widths, and data cut short: read as empty.
        for (width, height, len) in [(5, 3, 9), (4, 2, 4), (7, 4, 11), (1, 3, 2)] {
            let data: Vec<u8> = (0..len)
                .map(|i| (i as u8).wrapping_mul(97) ^ 0xa5)
                .collect();
            let glyph = GlyphRef {
                format: crate::glyph::COVERAGE4,
                left: 0,
                top: 0,
                width,
                height,
                advance: 0.0,
                data: &data,
            };
            let levels = glyph.levels();
            for y in 0..height + 1 {
                for x in 0..width + 1 {
                    let (wide_x, wide_y) = (u32::from(x), u32::from(y));
                    assert_eq!(
                        levels(wide_x, wide_y),
                        glyph.level(x, y),
                        "{width} ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn glyphs_fit_within_four_ems() {
        assert!(fits(80, 80, 10.0) && !fits(81, 1, 10.0));
        assert!(fits(32, 32, 1.0) && !fits(1, 33, 1.0));
        assert!(!fits(4096, 4096, 1000.0));
        assert!(!fits(1, 1, f32::NAN));
    }

    /// A glyph's rows in runs of any start and length are what `levels` and
    /// the tone give pixel by pixel, packed or not, past the data too.
    #[test]
    fn alpha_rows_are_the_levels_on_the_tone_in_every_run() {
        let tone: [u8; 16] = core::array::from_fn(|i| (i * 16 + 3) as u8);
        let mut tone = tone;
        tone[0] = 0;
        let mut state = 7u32;
        for (width, height) in [(1usize, 1usize), (2, 3), (7, 5), (16, 4), (33, 9)] {
            let stride = width.div_ceil(2);
            let mut raw = alloc::vec![0u8; stride * height];
            for byte in &mut raw {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *byte = (state >> 24) as u8;
            }
            let packed = crate::packed::encode(&raw, width as u16, height as u16).unwrap();
            for (format, data) in [
                (COVERAGE4, &raw[..]),
                (PACKED4, &packed[..]),
                (COVERAGE4, &raw[..raw.len() / 2]),
            ] {
                let glyph = GlyphRef {
                    format,
                    left: 0,
                    top: 0,
                    width: width as u16,
                    height: height as u16,
                    advance: 0.0,
                    data,
                };
                let level = glyph.levels();
                let rows = glyph.alpha_rows(tone);
                for dy in 0..height as u32 {
                    for dx in 0..width as u32 {
                        for len in 0..=(width as u32 - dx) as usize {
                            let mut out = alloc::vec![0xAAu8; len];
                            rows.fill(dy, dx, &mut out);
                            for (n, &alpha) in out.iter().enumerate() {
                                let want = tone[usize::from(level(dx + n as u32, dy) & 15)];
                                assert_eq!(alpha, want, "{width}x{height} {format} {dx}+{n},{dy}");
                            }
                        }
                    }
                }
            }
        }
    }
}
