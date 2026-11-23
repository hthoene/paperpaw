//! Packs a firmware carries with the bitmaps of some of their glyphs
//! deflated in blocks (#428).
//!
//! A pack made by [`PackBuilder`](crate::PackBuilder) is mostly bitmaps. For
//! a firmware's built-in packs, [`deflate_bitmaps`] rewrites one. The glyphs
//! the caller calls **cold** (a clock's large digits, the icons, letters few
//! texts use) have their bitmaps put at the end as one stream of bytes, cut
//! into blocks of [`BLOCK`] bytes and each block deflated on its own, so that
//! such a glyph is read by inflating the one block (rarely two) it lies in;
//! the glyph cache keeps what it reads. The other glyphs, which text is
//! drawn with, stay in place and are read in place as ever, and so does
//! everything else (the header, the strikes, the glyph records, kerning and
//! the tables): drawing and measuring text costs what it did and takes no
//! memory.
//!
//! ```text
//! the pack as made, with its cold bitmaps taken out and the hot ones after
//!                  the glyph records of all strikes (header, strikes, (directory),
//!                  records, bitmaps of hot glyphs, kerning and tables)
//! block table      u32 for each block and one more: where its deflated bytes
//!                  start in the blocks (the last: where they end)
//! blocks           raw deflate, no block longer than 4096 bytes inflated
//! trailer 16 bytes the size of the first part u32, the blocks' number u32,
//!                  the cold bitmaps' size u32, log2 of the block u16, FLAG u16
//! ```
//!
//! The pack's version byte has [`VERSION_FLAG`] set, so that a reader that does
//! not know it refuses it. A glyph record's data offset, as made, counts from
//! its strike's bitmaps; here, where its top bit [`COLD`] is set, from the
//! start of the cold stream (and the strike's bitmaps count from the first
//! hot one of the strike).

use alloc::vec::Vec;

#[cfg(feature = "kernel")]
use miniz_oxide::inflate::core::DecompressorOxide;

/// What a version byte has set when the pack's bitmaps are deflated.
pub(crate) const VERSION_FLAG: u8 = 0x80;
/// The top bit of a glyph record's data offset: its bitmap is in the cold
/// stream, and the offset the stream's.
pub(crate) const COLD: u32 = 0x8000_0000;
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
    /// bitmaps in the last) by the one decoder in `decoder`, made there on the
    /// first block: on the heap, as it is some 11 KB and no frame may hold
    /// it, and once for the blocks a glyph lies in. `false` where the block
    /// is damaged or there is no room.
    pub(crate) fn inflate(
        &self,
        data: &[u8],
        n: usize,
        decoder: &mut Vec<DecompressorOxide>,
        out: &mut Vec<u8>,
    ) -> bool {
        use miniz_oxide::inflate::TINFLStatus;
        use miniz_oxide::inflate::core::decompress;
        use miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;

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
        if decoder.is_empty() {
            if decoder.try_reserve_exact(1).is_err() {
                return false;
            }
            decoder.resize_with(1, DecompressorOxide::default);
        }
        let Some(state) = decoder.first_mut() else {
            return false;
        };
        // From the start of a stream: each block is deflated on its own.
        state.init();
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

/// How the builder laid a pack out (see [`layout`]).
struct Layout {
    version: u8,
    strikes: Vec<StrikeAt>,
    /// The end of the glyph records and bitmaps: where the kerning table and
    /// the tables begin, if there are any.
    tail: usize,
    kerning: usize,
}

/// A strike's place in the pack as made.
struct StrikeAt {
    glyphs: usize,
    table: usize,
    bitmaps: usize,
    size: f32,
}

/// What a pack made by the builder says of itself; `None` where it is not
/// one (cells are laid out differently) or is deflated.
fn layout(pack: &[u8]) -> Option<Layout> {
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
        if u16_at(pack, base + 2)? & 2 != 0
            || table.checked_add(glyphs.checked_mul(RECORD)?)? != bitmaps
        {
            return None;
        }
        let size = f32::from(u16_at(pack, base)?) / 4.0;
        strikes.push(StrikeAt {
            glyphs,
            table,
            bitmaps,
            size,
        });
    }
    // After the strikes: the kerning table and the tables, as far as there
    // are any.
    let kerning = u32_at(pack, KERNING)? as usize;
    let mut tail = pack.len();
    if kerning != 0 {
        tail = tail.min(kerning);
    }
    if version == 2 {
        for slot in 0..8 {
            let at = u32_at(pack, HEADER + count * STRIKE + slot * 4)? as usize;
            if at != 0 {
                tail = tail.min(at);
            }
        }
    }
    if tail < strikes.last()?.bitmaps || strikes.first()?.table != head {
        return None;
    }
    Some(Layout {
        version,
        strikes,
        tail,
        kerning,
    })
}

/// `pack`, made by [`PackBuilder`](crate::PackBuilder), with the bitmaps of
/// the glyphs `cold` says yes to (for a character and the size of its strike
/// in pixels) deflated in blocks (see the module's documentation) by
/// `deflate`, which compresses one block with no history, as raw deflate;
/// `None` where `pack` is not one as the builder lays them out (cells are not
/// supported) or is deflated already.
pub fn deflate_bitmaps(
    pack: &[u8],
    deflate: &dyn Fn(&[u8]) -> Vec<u8>,
    cold: &dyn Fn(char, f32) -> bool,
) -> Option<Vec<u8>> {
    let made = layout(pack)?;
    let count = made.strikes.len();
    let head = HEADER + count * STRIKE + if made.version == 2 { DIRECTORY } else { 0 };
    // Each glyph's bitmap is what lies between its offset and the next's in
    // the strike. The records go first, then the hot bitmaps of all strikes,
    // the cold ones in a stream of their own.
    let records: usize = made.strikes.iter().map(|s| s.glyphs * RECORD).sum();
    let flat = head + records;
    let mut out = pack.get(..head)?.to_vec();
    let (mut hot, mut stream, mut tables) = (Vec::new(), Vec::new(), Vec::new());
    for (i, strike) in made.strikes.iter().enumerate() {
        let end = made.strikes.get(i + 1).map_or(made.tail, |next| next.table);
        let base = HEADER + i * STRIKE;
        put(
            &mut out,
            base + 20,
            u32::try_from(head + tables.len()).ok()?,
        )?;
        put(&mut out, base + 24, u32::try_from(flat + hot.len()).ok()?)?;
        let first = hot.len();
        let area = pack.get(strike.bitmaps..end)?;
        let mut before = 0;
        for j in 0..strike.glyphs {
            let at = strike.table + j * RECORD;
            let mut record = pack.get(at..at + RECORD)?.to_vec();
            let from = u32_at(&record, 16)? as usize;
            let to = if j + 1 < strike.glyphs {
                u32_at(pack, at + RECORD + 16)? as usize
            } else {
                area.len()
            };
            // Offsets as the builder makes them: one glyph's bitmap after the
            // other's, none before the strike's.
            if from != before || to < from || to > area.len() {
                return None;
            }
            before = to;
            let bitmap = area.get(from..to)?;
            let ch = char::from_u32(u32_at(&record, 0)?).unwrap_or('\u{fffd}');
            let place = if cold(ch, strike.size) {
                let place = u32::try_from(stream.len()).ok().filter(|p| p & COLD == 0)?;
                stream.extend_from_slice(bitmap);
                place | COLD
            } else {
                let place = u32::try_from(hot.len() - first).ok()?;
                hot.extend_from_slice(bitmap);
                place
            };
            put(&mut record, 16, place)?;
            tables.extend_from_slice(&record);
        }
    }
    out.extend_from_slice(&tables);
    out.extend_from_slice(&hot);
    // The kerning table and the tables, where they are now.
    let moved = out.len();
    let shift = |at: usize| u32::try_from(moved + at - made.tail).ok();
    if made.kerning != 0 {
        put(&mut out, KERNING, shift(made.kerning)?)?;
    }
    if made.version == 2 {
        for slot in 0..8 {
            let at = HEADER + count * STRIKE + slot * 4;
            let old = u32_at(pack, at)? as usize;
            if old != 0 {
                put(&mut out, at, shift(old)?)?;
            }
        }
    }
    out.extend_from_slice(pack.get(made.tail..)?);
    out[4] = made.version | VERSION_FLAG;
    blocks(&mut out, &stream, deflate)?;
    Some(out)
}

/// The cold `stream` deflated in blocks after `out`, with the table of the
/// blocks and the trailer.
fn blocks(out: &mut Vec<u8>, stream: &[u8], deflate: &dyn Fn(&[u8]) -> Vec<u8>) -> Option<()> {
    let first = out.len();
    let blocks: Vec<Vec<u8>> = stream.chunks(BLOCK).map(deflate).collect();
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
    Some(())
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

    /// The letters of the packs below: 40 each from `A`.
    fn letter(i: u32) -> char {
        char::from_u32(0x41 + i).unwrap_or('?')
    }

    /// Strikes of 10, 20 and 30 px, 40 letters each, big enough for glyphs to lie across
    /// blocks, the last one with large ones.
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
                        letter(i),
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

    /// Which glyphs are cold in the tests: every third letter, and all of the largest strike.
    fn cold(ch: char, size: f32) -> bool {
        size > 25.0 || (ch as u32).is_multiple_of(3)
    }

    /// Every glyph of `plain` read through a glyph cache from `font` comes out as it
    /// does in place.
    fn same(plain: &BitmapFont, font: &Font, cache: &mut GlyphCache) -> usize {
        let mut checked = 0;
        for size in [10.0f32, 20.0, 30.0] {
            // In an order that goes back and forth over the blocks.
            for i in (0..40u32).chain((0..40).rev()) {
                let ch = letter(i);
                let a = plain.glyph(ch, size);
                let b = cache.glyph(0, font, ch, size);
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
        for (compact, tables) in [(false, false), (true, false), (true, true), (false, true)] {
            let flat = pack(compact, false, tables);
            let blocks = deflate_bitmaps(&flat, &deflate, &cold).unwrap();
            assert!(blocks.len() < flat.len(), "{compact} {tables}");
            let plain = BitmapFont::new(flat).unwrap();
            let font = BitmapFont::new(blocks).unwrap();
            assert!(font.is_deflated() && !plain.is_deflated());
            let mut cache = GlyphCache::new(4096);
            let deflated = Font::Bitmap(font.clone());
            assert_eq!(
                same(&plain, &deflated, &mut cache),
                240,
                "{compact} {tables}"
            );
            // Again, from a full cache.
            assert_eq!(
                same(&plain, &deflated, &mut cache),
                240,
                "{compact} {tables}"
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
    fn a_hot_glyph_is_read_in_place_and_costs_the_cache_nothing() {
        let flat = pack(true, false, false);
        let blocks = deflate_bitmaps(&flat, &deflate, &|_, size| size > 25.0).unwrap();
        let font = Font::Bitmap(BitmapFont::new(blocks).unwrap());
        let mut cache = GlyphCache::new(4096);
        for i in 0..40 {
            assert!(cache.glyph(0, &font, letter(i), 20.0).width > 0);
        }
        assert_eq!(cache.bytes(), 0);
        // A cold one is kept, once.
        assert!(cache.glyph(0, &font, 'B', 30.0).width > 0);
        assert!(cache.bytes() > 0);
    }

    #[test]
    fn a_large_glyph_is_not_kept_and_comes_out_whole() {
        let metrics = LineMetrics::fallback(20.0);
        let mut builder = PackBuilder::new();
        builder.compact();
        let big = |seed| noisy(120, 150, seed);
        builder.add_strike(
            120.0,
            metrics,
            vec![('0', big(1)), ('1', big(2)), ('2', big(3)), (':', big(4))],
        );
        let flat = builder.build();
        let blocks = deflate_bitmaps(&flat, &deflate, &|_, _| true).unwrap();
        let plain = BitmapFont::new(flat).unwrap();
        let font = Font::Bitmap(BitmapFont::new(blocks).unwrap());
        let mut cache = GlyphCache::new(4096);
        for ch in ['2', '0', ':', '1', '0'] {
            let a = plain.glyph(ch, 120.0);
            let b = cache.glyph(0, &font, ch, 120.0);
            assert_eq!((a.width, a.height), (b.width, b.height), "{ch}");
            let (la, lb) = (a.levels(), b.levels());
            for y in 0..a.height {
                for x in 0..a.width {
                    assert_eq!(
                        la(x.into(), y.into()),
                        lb(x.into(), y.into()),
                        "{ch} {x},{y}"
                    );
                }
            }
        }
        assert_eq!(cache.bytes(), 0, "a large glyph is held for one draw only");
    }

    #[test]
    fn each_block_inflates_to_its_size_and_none_past_the_last() {
        let blocks = deflate_bitmaps(&pack(true, false, false), &deflate, &cold).unwrap();
        let font = BitmapFont::new(blocks).unwrap();
        let (deflated, bytes) = font.deflated().unwrap();
        assert!(deflated.len > BLOCK, "{}", deflated.len);
        let (mut decoder, mut out) = (Vec::new(), Vec::new());
        assert!(deflated.inflate(bytes, 0, &mut decoder, &mut out) && out.len() == BLOCK);
        // The last block is what is left.
        let last = deflated.len / BLOCK;
        assert!(deflated.inflate(bytes, last, &mut decoder, &mut out));
        assert_eq!(out.len(), deflated.len % BLOCK);
        let past = deflated.len.div_ceil(BLOCK);
        assert!(!deflated.inflate(bytes, past, &mut decoder, &mut out));
    }

    #[test]
    fn a_damaged_pack_is_refused_or_drawn_as_nothing() {
        let blocks = deflate_bitmaps(&pack(true, false, true), &deflate, &cold).unwrap();
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
            for size in [10.0, 30.0] {
                let glyph = cache.glyph(0, &font, ch, size);
                assert_eq!(glyph.advance, plain.advance(ch, size), "{ch}");
            }
        }
    }

    /// A cold glyph whose record says it is larger than a glyph of its
    /// strike may be is not read (inflated, it would take as much memory as
    /// the record says), as a packed glyph is not.
    #[test]
    fn a_cold_glyph_larger_than_its_strike_allows_is_drawn_as_nothing() {
        let flat = pack(false, false, false);
        let mut blocks = deflate_bitmaps(&flat, &deflate, &cold).unwrap();
        let plain = BitmapFont::new(flat).unwrap();
        let b = plain.glyph('B', 10.0);
        assert!(cold('B', 10.0) && b.width > 0);
        // B's record at 10 px: its code, its size, a cold offset.
        let record = (0..blocks.len() - RECORD)
            .find(|&at| {
                u32_at(&blocks, at) == Some(u32::from('B'))
                    && u16_at(&blocks, at + 12) == Some(b.width)
                    && u16_at(&blocks, at + 14) == Some(b.height)
                    && u32_at(&blocks, at + 16).is_some_and(|at| at & COLD != 0)
            })
            .unwrap();
        // Wider than the 80 pixels a glyph at 10 px may be, one row: what it
        // says lies in the cold stream.
        blocks[record + 12..record + 16].copy_from_slice(&[81, 0, 1, 0]);
        let font = Font::Bitmap(BitmapFont::new(blocks).unwrap());
        let mut cache = GlyphCache::new(4096);
        let glyph = cache.glyph(0, &font, 'B', 10.0);
        assert_eq!((glyph.width, glyph.height), (0, 0));
    }

    #[test]
    fn a_pack_that_is_not_the_builders_or_is_deflated_already_is_not_deflated() {
        let flat = pack(false, false, false);
        assert!(deflate_bitmaps(b"PPFP", &deflate, &cold).is_none());
        // Cells are not laid out as this reads them.
        assert!(deflate_bitmaps(&pack(false, true, false), &deflate, &cold).is_none());
        let blocks = deflate_bitmaps(&flat, &deflate, &cold).unwrap();
        assert!(deflate_bitmaps(&blocks, &deflate, &cold).is_none());
    }

    #[test]
    fn a_pack_with_nothing_cold_is_the_same_pack_in_another_order() {
        let flat = pack(true, false, true);
        let blocks = deflate_bitmaps(&flat, &deflate, &|_, _| false).unwrap();
        let (plain, font) = (
            BitmapFont::new(flat).unwrap(),
            Font::Bitmap(BitmapFont::new(blocks).unwrap()),
        );
        let mut cache = GlyphCache::new(4096);
        assert_eq!(same(&plain, &font, &mut cache), 240);
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn a_bitmap_font_without_inflating_refuses_nothing_it_can_read() {
        // A flat pack reads as ever, whatever the feature.
        let font = BitmapFont::new(pack(false, false, false)).unwrap();
        assert!(!font.is_deflated());
        assert!(!font.glyph('A', 10.0).is_empty());
    }
}
