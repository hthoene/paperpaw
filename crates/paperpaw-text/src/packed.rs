//! Compact storage of a glyph's bitmap in a pack (the strike flag
//! [`crate::bitmap::STRIKE_PACKED`], Decision 197): the same 4-bit coverage as
//! [`crate::glyph::COVERAGE4`], read back bit for bit, in about half the bytes.
//!
//! A glyph is a stream of **nibbles**, high nibble of a byte first, for the
//! pixels row by row with no padding at the end of a row (the glyph's
//! last byte is padded with a zero nibble):
//!
//! ```text
//! 1..=14        one pixel of that level
//! 0 or 15, r    a run of pixels of level 0 (or 15); r in 0..=14: r + 1 pixels
//! 0 or 15, 15, hi, lo   r = 15: the run is 16 + (hi * 16 + lo) pixels (16 to 271)
//! ```
//!
//! Most of a glyph is empty (0) or full ink (15), edges are the rest: the
//! UI font and icons shrink to about 50 %. A longer run is written as several.
//!
//! The reader decodes **one row at a time** into [`Rows`]' buffer (no heap:
//! [`MAX_WIDTH`] bytes), rows asked for in order as drawing does; asking for an
//! earlier row starts the glyph again. A glyph wider than [`MAX_WIDTH`] is not
//! packed (its strike stays as it is).

use core::cell::Cell;

/// The widest glyph that is packed, in pixels: the buffer of [`Rows`].
pub(crate) const MAX_WIDTH: usize = 192;

/// The longest run one token holds.
const LONGEST: usize = 16 + 255;

/// Encodes a glyph's 4-bit coverage (two pixels per byte, high nibble
/// first, rows padded to bytes, as [`crate::glyph::COVERAGE4`]) of `width` and
/// `height`; `None` where it is too wide to be packed. Data too short for the
/// glyph reads as empty, as everywhere.
pub(crate) fn encode(data: &[u8], width: u16, height: u16) -> Option<alloc::vec::Vec<u8>> {
    use alloc::vec::Vec;
    let (w, h) = (usize::from(width), usize::from(height));
    if w > MAX_WIDTH {
        return None;
    }
    let stride = w.div_ceil(2);
    let level = |x: usize, y: usize| -> u8 {
        let byte = data.get(y * stride + x / 2).copied().unwrap_or(0);
        if x.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 15
        }
    };
    let mut nibbles: Vec<u8> = Vec::new();
    let total = w * h;
    let at = |i: usize| level(i % w.max(1), i / w.max(1));
    let mut i = 0;
    while i < total {
        let v = at(i);
        if v == 0 || v == 15 {
            let mut run = 1;
            while i + run < total && at(i + run) == v {
                run += 1;
            }
            i += run;
            while run > 0 {
                if run <= 15 {
                    nibbles.extend_from_slice(&[v, (run - 1) as u8]);
                    run = 0;
                } else {
                    let n = run.min(LONGEST);
                    let extra = n - 16;
                    nibbles.extend_from_slice(&[v, 15, (extra >> 4) as u8, (extra & 15) as u8]);
                    run -= n;
                }
            }
        } else {
            nibbles.push(v);
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(nibbles.len().div_ceil(2));
    for pair in nibbles.chunks(2) {
        out.push(pair[0] << 4 | pair.get(1).copied().unwrap_or(0));
    }
    Some(out)
}

/// A packed glyph read row by row: the glyph's stream, where the decoding of
/// it has got to and the row last decoded.
#[derive(Debug)]
pub(crate) struct Rows<'a> {
    data: &'a [u8],
    width: usize,
    height: u32,
    /// The index of the row in `buf`, `u32::MAX` before the first.
    row: Cell<u32>,
    /// The next nibble of the stream.
    next: Cell<usize>,
    /// The level and the length left of the run being read.
    level: Cell<u8>,
    left: Cell<u16>,
    buf: [Cell<u8>; MAX_WIDTH],
}

impl<'a> Rows<'a> {
    /// A glyph of `width` and `height` in `data`; one wider than
    /// [`MAX_WIDTH`] reads as empty.
    pub(crate) fn new(data: &'a [u8], width: u16, height: u16) -> Self {
        let usable = usize::from(width) <= MAX_WIDTH;
        Self {
            data,
            width: usize::from(width),
            height: if usable { u32::from(height) } else { 0 },
            row: Cell::new(u32::MAX),
            next: Cell::new(0),
            level: Cell::new(0),
            left: Cell::new(0),
            buf: [const { Cell::new(0) }; MAX_WIDTH],
        }
    }

    /// The level of pixel (`x`, `y`) as stored, 0 beyond the glyph.
    #[inline]
    pub(crate) fn level(&self, x: u32, y: u32) -> u8 {
        if y != self.row.get() && !self.seek(y) {
            return 0;
        }
        self.buf.get(x as usize).map_or(0, Cell::get)
    }

    /// Has row `y` in the buffer, decoding from the row after the one there
    /// (from the start for an earlier one); false beyond the glyph.
    #[inline(never)]
    fn seek(&self, y: u32) -> bool {
        if y >= self.height {
            return false;
        }
        let have = self.row.get();
        if have == u32::MAX || y < have {
            self.row.set(u32::MAX);
            self.next.set(0);
            self.left.set(0);
        }
        let mut row = self.row.get().wrapping_add(1);
        while row <= y {
            for cell in self.buf.iter().take(self.width) {
                cell.set(self.pixel());
            }
            self.row.set(row);
            row += 1;
        }
        true
    }

    fn nibble(&self) -> u8 {
        let n = self.next.get();
        self.next.set(n + 1);
        match self.data.get(n / 2) {
            Some(&byte) if n.is_multiple_of(2) => byte >> 4,
            Some(&byte) => byte & 15,
            None => 0,
        }
    }

    fn pixel(&self) -> u8 {
        if self.left.get() > 0 {
            self.left.set(self.left.get() - 1);
            return self.level.get();
        }
        let v = self.nibble();
        if v != 0 && v != 15 {
            return v;
        }
        let r = self.nibble();
        let len = if r == 15 {
            let hi = u16::from(self.nibble());
            16 + hi * 16 + u16::from(self.nibble())
        } else {
            u16::from(r) + 1
        };
        self.level.set(v);
        self.left.set(len - 1);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Two pixels per byte, rows padded, as the packs store them.
    fn raw(levels: &[u8], width: usize) -> Vec<u8> {
        let height = levels.len() / width.max(1);
        let stride = width.div_ceil(2);
        let mut out = vec![0; stride * height];
        for (i, &v) in levels.iter().enumerate() {
            let (x, y) = (i % width, i / width);
            out[y * stride + x / 2] |= if x % 2 == 0 { v << 4 } else { v };
        }
        out
    }

    fn round_trip(levels: &[u8], width: usize) {
        let height = levels.len() / width;
        let bytes = raw(levels, width);
        let packed = encode(&bytes, width as u16, height as u16).unwrap();
        let rows = Rows::new(&packed, width as u16, height as u16);
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    rows.level(x as u32, y as u32),
                    levels[y * width + x],
                    "{x},{y} of {width}x{height}"
                );
            }
        }
        // Asked again in another order: from the start where a row before.
        for y in (0..height).rev() {
            for x in (0..width).rev() {
                assert_eq!(rows.level(x as u32, y as u32), levels[y * width + x]);
            }
        }
    }

    /// A deterministic mix of empty, full and edge pixels.
    fn noise(len: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match (s >> 24) % 8 {
                    0..=3 => 0,
                    4..=5 => 15,
                    _ => ((s >> 16) % 16) as u8,
                }
            })
            .collect()
    }

    #[test]
    fn glyphs_of_every_shape_read_back_bit_for_bit() {
        for width in [1, 2, 3, 15, 16, 17, 31, 100, 191, 192] {
            for height in [1, 2, 7, 40] {
                round_trip(&noise(width * height, (width * 31 + height) as u32), width);
            }
        }
    }

    #[test]
    fn long_runs_of_empty_and_full_are_cut_at_every_length() {
        for run in [
            1, 2, 14, 15, 16, 17, 30, 31, 32, 270, 271, 272, 273, 500, 1000,
        ] {
            for value in [0, 15, 7] {
                let mut levels = vec![value; run];
                levels.push(3);
                levels.extend(vec![15 - value % 15; run]);
                // One column, then the same cut into rows of 10.
                round_trip(&levels, 1);
                levels.resize(levels.len().div_ceil(10) * 10, 5);
                round_trip(&levels, 10);
            }
        }
    }

    #[test]
    fn a_blank_glyph_and_one_too_wide_pack_to_nothing_or_stay_unpacked() {
        assert_eq!(encode(&[], 0, 0), Some(vec![]));
        assert_eq!(encode(&[], 5, 0), Some(vec![]));
        assert!(encode(&vec![0; 100 * 200], 200, 100).is_none());
        let rows = Rows::new(&[], 200, 100);
        assert_eq!(rows.level(0, 0), 0);
    }

    #[test]
    fn truncated_data_reads_as_empty_and_does_not_panic() {
        let levels = noise(30 * 30, 9);
        let bytes = raw(&levels, 30);
        let packed = encode(&bytes, 30, 30).unwrap();
        for cut in [0, 1, packed.len() / 2, packed.len() - 1] {
            let rows = Rows::new(&packed[..cut], 30, 30);
            for y in 0..31 {
                for x in 0..31 {
                    let _ = rows.level(x, y);
                }
            }
        }
        // Beyond the glyph: empty.
        let rows = Rows::new(&packed, 30, 30);
        assert_eq!(rows.level(30, 0), 0);
        assert_eq!(rows.level(0, 30), 0);
    }

    #[test]
    fn a_packed_glyph_is_about_half_of_a_typical_one() {
        // A line-art glyph: mostly empty, a few edge rows.
        let mut levels = vec![0; 40 * 40];
        for y in 10..30 {
            for x in 18..22 {
                levels[y * 40 + x] = if x == 18 || x == 21 { 7 } else { 15 };
            }
        }
        let packed = encode(&raw(&levels, 40), 40, 40).unwrap();
        assert!(packed.len() * 4 < 40 * 20, "{} bytes", packed.len());
    }
}
