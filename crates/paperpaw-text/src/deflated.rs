//! Packs a firmware carries with their bitmaps deflated in blocks (#428).
//!
//! A pack made by [`PackBuilder`](crate::PackBuilder) is mostly bitmaps. For
//! a firmware's built-in packs, [`deflate_bitmaps`] rewrites one: the bitmaps
//! of all strikes are put at the end as one stream of bytes, cut into blocks
//! of [`BLOCK`] bytes and each block deflated on its own, so that a glyph is
//! read by inflating the one block (rarely two) it lies in. Everything else
//! (the header, the strikes, the glyph records, kerning and the tables) stays
//! in place and is read in place as ever, so that measuring text costs what it
//! did.
//!
//! ```text
//! the pack as made, with its bitmaps taken out:   header, strikes, (directory),
//!                                                 the strikes' glyph records,
//!                                                 kerning and tables
//! block table      u32 for each block and one more: where its deflated bytes
//!                  start in the blocks (the last: where they end)
//! blocks           raw deflate, no block longer than 4096 bytes inflated
//! trailer 16 bytes the size of the first part u32, the blocks' number u32,
//!                  the bitmaps' size u32, log2 of the block u16, FLAG u16
//! ```
//!
//! The pack's version byte has [`VERSION_FLAG`] set, so that a reader that does
//! not know it refuses it. A strike's bitmap offset counts from the start of
//! the bitmaps' stream; a glyph record's data offset from the strike's, as in
//! a pack made flat.

use alloc::vec::Vec;

/// What a version byte has set when the pack's bitmaps are deflated.
pub(crate) const VERSION_FLAG: u8 = 0x80;
/// The bytes of bitmaps one block holds inflated.
pub(crate) const BLOCK: usize = 4096;
/// The trailer's size and its mark, the last two bytes.
#[cfg(feature = "kernel")]
const TRAILER: usize = 16;
const MARK: u16 = 0x5a42;
/// Where the header, the strike headers, and (version 2) the directory are:
/// as `bitmap.rs` lays them out.
const HEADER: usize = 12;
const STRIKE: usize = 32;
const RECORD: usize = 20;
const DIRECTORY: usize = 32;
/// The offset of the kerning table in the first strike's header.
const KERNING: usize = HEADER + 28;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn put(data: &mut [u8], at: usize, value: u32) -> Option<()> {
    data.get_mut(at..at.checked_add(4)?)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

/// Where a pack's bitmaps are when they are deflated: found by
/// [`Deflated::of`]. Read only where the program inflates (the kernel's).
#[cfg(feature = "kernel")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deflated {
    /// The bytes of bitmaps, inflated.
    pub(crate) len: usize,
    /// The block table's place in the pack.
    table: usize,
    blocks: usize,
}

#[cfg(feature = "kernel")]
impl Deflated {
    /// What `data`, a pack whose version byte has [`VERSION_FLAG`], says of
    /// its blocks; `None` where its trailer does not describe it.
    pub(crate) fn of(data: &[u8]) -> Option<Deflated> {
        let trailer = data.len().checked_sub(TRAILER)?;
        if u16_at(data, trailer + 14)? != MARK
            || usize::from(u16_at(data, trailer + 12)?) != BLOCK.ilog2() as usize
        {
            return None;
        }
        let table = u32_at(data, trailer)? as usize;
        let blocks = u32_at(data, trailer + 4)? as usize;
        let len = u32_at(data, trailer + 8)? as usize;
        let table_end = blocks.checked_add(1)?.checked_mul(4)?.checked_add(table)?;
        // The blocks' number is what the bitmaps' size needs, the table fits.
        if blocks != len.div_ceil(BLOCK) || table_end > trailer {
            return None;
        }
        Some(Deflated { len, table, blocks })
    }

    /// Block `n` inflated into `out` (`BLOCK` bytes, or what is left of the
    /// bitmaps in the last): `false` where it is damaged or there is no room.
    pub(crate) fn inflate(&self, data: &[u8], n: usize, out: &mut Vec<u8>) -> bool {
        use miniz_oxide::inflate::TINFLStatus;
        use miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
        use miniz_oxide::inflate::core::{DecompressorOxide, decompress};

        if n >= self.blocks {
            return false;
        }
        let blob = self.table + (self.blocks + 1) * 4;
        let (Some(from), Some(to)) = (
            u32_at(data, self.table + n * 4),
            u32_at(data, self.table + n * 4 + 4),
        ) else {
            return false;
        };
        let Some(packed) = (from <= to)
            .then(|| data.get(blob + from as usize..blob + to as usize))
            .flatten()
        else {
            return false;
        };
        let size = (self.len - n * BLOCK).min(BLOCK);
        out.clear();
        if out.try_reserve_exact(size).is_err() {
            return false;
        }
        out.resize(size, 0);
        // On the heap: the decoder is some 11 KB, no frame may hold it.
        let mut decoder: Vec<DecompressorOxide> = Vec::new();
        if decoder.try_reserve_exact(1).is_err() {
            return false;
        }
        decoder.resize_with(1, DecompressorOxide::default);
        let Some(state) = decoder.first_mut() else {
            return false;
        };
        let (status, _, made) = decompress(
            state,
            packed,
            out,
            0,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        );
        status == TINFLStatus::Done && made == size
    }
}

/// `pack`, made by [`PackBuilder`](crate::PackBuilder), with its bitmaps
/// deflated in blocks (see the module's documentation) by `deflate`, which
/// compresses one block with no history, as raw deflate; `None` where `pack`
/// is not one as the builder lays them out (or is deflated already).
pub fn deflate_bitmaps(pack: &[u8], deflate: &dyn Fn(&[u8]) -> Vec<u8>) -> Option<Vec<u8>> {
    let version = *pack.get(4)?;
    if !pack.starts_with(b"PPFP") || !(version == 1 || version == 2) {
        return None;
    }
    let count = usize::from(u16_at(pack, 8)?);
    let head = HEADER + count * STRIKE + if version == 2 { DIRECTORY } else { 0 };
    // Where each strike's glyph records and bitmaps are, which the builder
    // lays out one strike after the other.
    let mut strikes = Vec::new();
    for i in 0..count {
        let base = HEADER + i * STRIKE;
        let glyphs = u32_at(pack, base + 16)? as usize;
        let (table, bitmaps) = (
            u32_at(pack, base + 20)? as usize,
            u32_at(pack, base + 24)? as usize,
        );
        if table.checked_add(glyphs.checked_mul(RECORD)?)? != bitmaps {
            return None;
        }
        strikes.push((table, bitmaps));
    }
    // After the strikes: the kerning table and the tables, as far as there
    // are any.
    let mut tail = pack.len();
    let kerning = u32_at(pack, KERNING)? as usize;
    let mut offsets = Vec::new();
    if kerning != 0 {
        offsets.push(kerning);
    }
    if version == 2 {
        for slot in 0..8 {
            let at = u32_at(pack, HEADER + count * STRIKE + slot * 4)? as usize;
            if at != 0 {
                offsets.push(at);
            }
        }
    }
    for &at in &offsets {
        tail = tail.min(at);
    }
    let last = strikes.last()?.1;
    if tail < last || strikes.first()?.0 != head {
        return None;
    }
    // The new first part: what was before the glyph records, the records,
    // and the tail; the bitmaps in a stream of their own.
    let mut out = pack.get(..head)?.to_vec();
    let mut stream = Vec::new();
    for (i, &(table, bitmaps)) in strikes.iter().enumerate() {
        let end = strikes.get(i + 1).map_or(tail, |next| next.0);
        let base = HEADER + i * STRIKE;
        let here = u32::try_from(out.len()).ok()?;
        put(&mut out, base + 20, here)?;
        put(&mut out, base + 24, u32::try_from(stream.len()).ok()?)?;
        out.extend_from_slice(pack.get(table..bitmaps)?);
        stream.extend_from_slice(pack.get(bitmaps..end)?);
    }
    let moved = out.len();
    let shift = |at: usize| u32::try_from(moved + at - tail).ok();
    if kerning != 0 {
        put(&mut out, KERNING, shift(kerning)?)?;
    }
    if version == 2 {
        for slot in 0..8 {
            let at = HEADER + count * STRIKE + slot * 4;
            let old = u32_at(pack, at)? as usize;
            if old != 0 {
                put(&mut out, at, shift(old)?)?;
            }
        }
    }
    out.extend_from_slice(pack.get(tail..)?);
    out[4] = version | VERSION_FLAG;
    let first = out.len();
    let mut blocks = Vec::new();
    for chunk in stream.chunks(BLOCK) {
        blocks.push(deflate(chunk));
    }
    let mut at = 0u32;
    for block in &blocks {
        out.extend_from_slice(&at.to_le_bytes());
        at = at.checked_add(u32::try_from(block.len()).ok()?)?;
    }
    out.extend_from_slice(&at.to_le_bytes());
    for block in &blocks {
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&u32::try_from(first).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(blocks.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(stream.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&(BLOCK.ilog2() as u16).to_le_bytes());
    out.extend_from_slice(&MARK.to_le_bytes());
    Some(out)
}

#[cfg(all(test, feature = "kernel"))]
mod tests {
    use super::*;
    use crate::bitmap::{BitmapFont, PackBuilder};
    use crate::cache::GlyphCache;
    use crate::font::{Font, FontError, LineMetrics};
    use crate::glyph::{COVERAGE4, Glyph};
    use alloc::vec;

    /// A glyph of deterministic levels: empty, full and edge pixels.
    fn noisy(width: u16, height: u16, seed: u32) -> Glyph {
        let (mut s, mut run, mut level) = (seed, 0, 0u8);
        let stride = usize::from(width).div_ceil(2);
        let mut data = vec![0u8; stride * usize::from(height)];
        for i in 0..usize::from(width) * usize::from(height) {
            if run == 0 {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                level = match (s >> 24) % 8 {
                    0..=3 => 0,
                    4..=5 => 15,
                    n => (n * 2) as u8,
                };
                run = 1 + (s >> 16) % 24;
            }
            run -= 1;
            let (x, y) = (i % usize::from(width), i / usize::from(width));
            data[y * stride + x / 2] |= if x % 2 == 0 { level << 4 } else { level };
        }
        Glyph {
            format: COVERAGE4,
            left: 1,
            top: -2,
            width,
            height,
            advance: f32::from(width) + 1.5,
            data,
        }
    }

    fn deflate(block: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec(block, 9)
    }

    /// Strikes of 40 letters each, big enough for glyphs to lie across blocks.
    fn pack(compact: bool, cells: bool, tables: bool) -> Vec<u8> {
        let metrics = LineMetrics::fallback(20.0);
        let mut builder = PackBuilder::new();
        if compact {
            builder.compact();
        }
        if cells {
            builder.cells();
        }
        for (n, size) in [10.0f32, 20.0, 30.0].into_iter().enumerate() {
            let glyphs = (0..40u32)
                .map(|i| {
                    let (w, h) = (
                        4 + (i as u16 * 7 + n as u16 * 3) % 20,
                        5 + (i as u16 * 5) % 24,
                    );
                    (
                        char::from_u32(0x41 + i).unwrap_or('?'),
                        noisy(w * (n as u16 + 1), h * (n as u16 + 1), i * 31 + n as u32),
                    )
                })
                .collect();
            builder.add_strike(size, metrics, glyphs);
        }
        builder.kerning(&[('A', 'B', -0.05), ('C', 'D', 0.04)]);
        if tables {
            builder.table(BitmapFont::SCRIPTS, 1, &[1, 0, 0, 0, 7, 8, 9]);
        }
        builder.build()
    }

    /// Every glyph of `plain` read through a glyph cache from `deflated`
    /// comes out as it does in place.
    fn same(plain: &BitmapFont, deflated: Font, cache: &mut GlyphCache) -> usize {
        let mut checked = 0;
        for size in [10.0f32, 20.0, 30.0] {
            // In an order that goes back and forth over the blocks.
            for i in (0..40u32).chain((0..40).rev()) {
                let ch = char::from_u32(0x41 + i).unwrap_or('?');
                let a = plain.glyph(ch, size);
                let b = cache.glyph(0, &deflated, ch, size);
                assert_eq!(
                    (a.width, a.height, a.left, a.top, a.advance),
                    (b.width, b.height, b.left, b.top, b.advance),
                    "{ch} at {size}"
                );
                let (la, lb) = (a.levels(), b.levels());
                for y in 0..a.height {
                    for x in 0..a.width {
                        assert_eq!(
                            la(u32::from(x), u32::from(y)),
                            lb(u32::from(x), u32::from(y)),
                            "{ch} at {size}: pixel {x},{y}"
                        );
                    }
                }
                checked += 1;
            }
        }
        checked
    }

    #[test]
    fn deflated_bitmaps_give_every_pixel_of_the_pack_made_flat() {
        for (compact, cells, tables) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, false, true),
            (false, false, true),
        ] {
            let flat = pack(compact, cells, tables);
            let blocks = deflate_bitmaps(&flat, &deflate).unwrap();
            assert!(blocks.len() < flat.len(), "{compact} {cells} {tables}");
            let plain = BitmapFont::new(flat).unwrap();
            let font = BitmapFont::new(blocks).unwrap();
            assert!(font.is_deflated() && !plain.is_deflated());
            let mut cache = GlyphCache::new(4096);
            assert_eq!(
                same(&plain, Font::Bitmap(font.clone()), &mut cache),
                240,
                "{compact} {cells} {tables}"
            );
            // What measuring asks stays in place.
            assert_eq!(
                font.sizes().collect::<Vec<_>>(),
                plain.sizes().collect::<Vec<_>>()
            );
            assert_eq!(font.kern('A', 'B', 20.0), plain.kern('A', 'B', 20.0));
            assert_eq!(font.advance('C', 20.0), plain.advance('C', 20.0));
            assert_eq!(
                font.table(BitmapFont::SCRIPTS),
                plain.table(BitmapFont::SCRIPTS)
            );
        }
    }

    #[test]
    fn a_block_is_inflated_once_for_the_glyphs_in_it() {
        let blocks = deflate_bitmaps(&pack(true, false, false), &deflate).unwrap();
        let font = BitmapFont::new(blocks).unwrap();
        let (deflated, bytes) = font.deflated().unwrap();
        assert!(deflated.len > BLOCK * 3, "{}", deflated.len);
        let mut out = Vec::new();
        assert!(deflated.inflate(bytes, 0, &mut out) && out.len() == BLOCK);
        // The last block is what is left.
        assert!(deflated.inflate(bytes, deflated.len / BLOCK, &mut out));
        assert_eq!(out.len(), deflated.len % BLOCK);
        assert!(!deflated.inflate(bytes, deflated.len.div_ceil(BLOCK), &mut out));
    }

    #[test]
    fn a_damaged_pack_is_refused_or_drawn_as_nothing() {
        let blocks = deflate_bitmaps(&pack(true, false, true), &deflate).unwrap();
        // The trailer: damaged, no pack.
        for at in [1usize, 2, 3, 4, 9, 10, 11, 12] {
            let mut bad = blocks.clone();
            let end = bad.len();
            bad[end - at] ^= 0x55;
            assert!(BitmapFont::new(bad).is_err(), "trailer byte {at}");
        }
        assert_eq!(
            BitmapFont::new(blocks[..blocks.len() - 1].to_vec()).unwrap_err(),
            FontError::Invalid
        );
        // A block damaged: the glyph in it is nothing, its advance is as it was.
        let mut cache = GlyphCache::new(4096);
        let plain = BitmapFont::new(pack(true, false, true)).unwrap();
        let mut bad = blocks.clone();
        let table = BitmapFont::new(blocks.clone()).unwrap();
        let (deflated, _) = table.deflated().unwrap();
        let blob = deflated.table + (deflated.blocks + 1) * 4;
        for byte in &mut bad[blob..blob + 40] {
            *byte ^= 0xff;
        }
        let font = Font::Bitmap(BitmapFont::new(bad).unwrap());
        for ch in ('A'..='Z').chain('a'..='z') {
            let glyph = cache.glyph(0, &font, ch, 10.0);
            assert_eq!(glyph.advance, plain.advance(ch, 10.0), "{ch}");
        }
    }

    #[test]
    fn a_pack_that_is_not_the_builders_or_is_deflated_already_is_not_deflated() {
        let flat = pack(false, false, false);
        assert!(deflate_bitmaps(b"PPFP", &deflate).is_none());
        let blocks = deflate_bitmaps(&flat, &deflate).unwrap();
        assert!(deflate_bitmaps(&blocks, &deflate).is_none());
    }

    #[test]
    fn a_bitmap_font_without_inflating_refuses_nothing_it_can_read() {
        // A flat pack reads as ever, whatever the feature.
        let font = BitmapFont::new(pack(false, false, false)).unwrap();
        assert!(!font.is_deflated());
        assert!(!font.glyph('A', 10.0).is_empty());
    }
}
