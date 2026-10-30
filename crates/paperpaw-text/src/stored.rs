//! Font packs read where they are kept (e.g. on an SD card), a glyph at a
//! time, for scripts a device's built-in packs lack: only the characters
//! a pack holds and, for each size in use, their advances stay in memory
//! (a few hundred bytes each); glyphs are read when first drawn and kept
//! in the [`GlyphCache`](crate::GlyphCache) like rasterized ones.
//!
//! The format is the packs' (see [`BitmapFont`](crate::BitmapFont));
//! every strike must hold the same characters, as `paperpaw-fontgen`
//! makes them. What a damaged pack says is checked before memory is set
//! aside for it: a glyph larger than its size allows reads as empty.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use crate::font::{FontError, LineMetrics};
use crate::glyph::{self, Glyph, zeroed};

const MAGIC: &[u8; 4] = b"PPFP";
/// Packs made before PaperPaw had its name (the same format): still read.
const OLD_MAGIC: &[u8; 4] = b"QIFP"; // before PPFP
const VERSION: u8 = 1;
/// A pack with a directory of tables (see [`StoredPack::table`]).
const VERSION_TABLES: u8 = 2;
const HEADER: usize = 12;
const STRIKE: usize = 32;
const RECORD: usize = 20;
/// Characters a pack may hold at most (a script or a few).
const MAX_CHARS: usize = 8192;
/// Sizes a pack may hold at most.
const MAX_STRIKES: usize = 64;

/// Where a pack's bytes come from.
pub trait PackSource {
    /// Fills `buf` from `at`; `false` if that could not be read whole.
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool;
}

/// What a strike's header says.
#[derive(Debug, Clone, Copy)]
struct Strike {
    /// Quarter pixels.
    size: u16,
    metrics: LineMetrics,
    glyph_table: u64,
    bitmaps: u64,
}

/// A font pack read from a [`PackSource`].
#[derive(Clone)]
pub struct StoredPack {
    source: Rc<dyn PackSource>,
    strikes: Vec<Strike>,
    /// The characters it holds, sorted (the same in every strike).
    chars: Vec<char>,
    /// Each strike's advances in 1/16 px, once it was used.
    advances: RefCell<Vec<Option<Vec<u16>>>>,
    /// The offsets of its tables, 0 where absent (see
    /// [`StoredPack::table`]).
    tables: [u32; 8],
}

impl fmt::Debug for StoredPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredPack")
            .field("strikes", &self.strikes.len())
            .field("chars", &self.chars.len())
            .finish_non_exhaustive()
    }
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn i16_le(b: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn i32_le(b: &[u8], at: usize) -> i32 {
    u32_le(b, at) as i32
}

fn q6(value: i32) -> f32 {
    value as f32 / 64.0
}

impl StoredPack {
    /// Reads the pack's header and its characters.
    pub fn open(source: Rc<dyn PackSource>) -> Result<StoredPack, FontError> {
        let mut header = [0u8; HEADER];
        if !source.read_at(0, &mut header) || (&header[..4] != MAGIC && &header[..4] != OLD_MAGIC) {
            return Err(FontError::Invalid);
        }
        if header[4] != VERSION && header[4] != VERSION_TABLES {
            return Err(FontError::UnsupportedVersion);
        }
        let count = usize::from(u16_le(&header, 8));
        if count == 0 || count > MAX_STRIKES {
            return Err(FontError::Invalid);
        }
        let mut table = alloc::vec![0u8; count * STRIKE];
        if !source.read_at(HEADER as u64, &mut table) {
            return Err(FontError::Invalid);
        }
        let mut strikes = Vec::with_capacity(count);
        let mut glyphs = None;
        for s in table.as_chunks::<STRIKE>().0 {
            let n = u32_le(s, 16) as usize;
            // The same characters in every strike.
            if n > MAX_CHARS || glyphs.is_some_and(|g| g != n) {
                return Err(FontError::Invalid);
            }
            glyphs = Some(n);
            strikes.push(Strike {
                size: u16_le(s, 0),
                metrics: LineMetrics {
                    ascent: q6(i32_le(s, 4)),
                    descent: q6(i32_le(s, 8)),
                    line_height: q6(i32_le(s, 12)),
                },
                glyph_table: u64::from(u32_le(s, 20)),
                bitmaps: u64::from(u32_le(s, 24)),
            });
        }
        let n = glyphs.unwrap_or(0);
        let mut records = zeroed(n * RECORD).ok_or(FontError::Invalid)?;
        if !source.read_at(strikes[0].glyph_table, &mut records) {
            return Err(FontError::Invalid);
        }
        let chars: Vec<char> = records
            .as_chunks::<RECORD>()
            .0
            .iter()
            .filter_map(|r| char::from_u32(u32_le(r, 0)))
            .collect();
        if chars.len() != n || chars.windows(2).any(|w| w[0] >= w[1]) {
            return Err(FontError::Invalid);
        }
        let mut tables = [0u32; 8];
        if header[4] == VERSION_TABLES {
            let mut directory = [0u8; 32];
            if !source.read_at((HEADER + count * STRIKE) as u64, &mut directory) {
                return Err(FontError::Invalid);
            }
            for (slot, offset) in tables.iter_mut().zip(directory.as_chunks::<4>().0) {
                *slot = u32::from_le_bytes(*offset);
            }
        }
        Ok(StoredPack {
            source,
            advances: RefCell::new(alloc::vec![None; strikes.len()]),
            strikes,
            chars,
            tables,
        })
    }

    /// Where table `slot` (0 to 7, see
    /// [`BitmapFont::ANCHORS`](crate::BitmapFont::ANCHORS)) starts in the
    /// pack, if it has one: read it through its [`PackSource`]. Slot 7
    /// is the next directory, of slots 8 to 15.
    pub fn table(&self, slot: usize) -> Option<u64> {
        self.tables
            .get(slot)
            .filter(|&&at| at != 0)
            .map(|&at| u64::from(at))
    }

    /// The largest size it has, in pixels (every strike has every
    /// character).
    pub fn largest_size(&self) -> Option<f32> {
        self.strikes
            .iter()
            .map(|s| f32::from(s.size) / 4.0)
            .reduce(f32::max)
    }

    /// The strike closest to `size`.
    fn nearest(&self, size: f32) -> usize {
        let wanted = (size * 4.0) as i32;
        (0..self.strikes.len())
            .min_by_key(|&i| i32::from(self.strikes[i].size).abs_diff(wanted))
            .unwrap_or(0)
    }

    pub fn has_char(&self, ch: char) -> bool {
        self.chars.binary_search(&ch).is_ok()
    }

    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        self.strikes[self.nearest(size)].metrics
    }

    /// The advance of `ch` at the strike nearest to `size`; 0 if it is
    /// not in the pack.
    pub fn advance(&self, ch: char, size: f32) -> f32 {
        let Ok(index) = self.chars.binary_search(&ch) else {
            return 0.0;
        };
        let strike = self.nearest(size);
        let mut advances = self.advances.borrow_mut();
        if advances[strike].is_none() {
            // A strike's advances, read once (tried again while memory is
            // short).
            let Some(mut records) = zeroed(self.chars.len() * RECORD) else {
                return 0.0;
            };
            let read = self
                .source
                .read_at(self.strikes[strike].glyph_table, &mut records);
            advances[strike] = Some(
                records
                    .as_chunks::<RECORD>()
                    .0
                    .iter()
                    .map(|r| {
                        if read {
                            (i32_le(r, 4) / 4).clamp(0, i32::from(u16::MAX)) as u16
                        } else {
                            0
                        }
                    })
                    .collect(),
            );
        }
        advances[strike]
            .as_ref()
            .and_then(|a| a.get(index))
            .map_or(0.0, |&a| f32::from(a) / 16.0)
    }

    /// The glyph of `ch` at the strike nearest to `size`, read from the
    /// source; empty if it is not in the pack or cannot be read.
    pub fn render(&self, ch: char, size: f32) -> Glyph {
        let Ok(index) = self.chars.binary_search(&ch) else {
            return Glyph::empty(0.0);
        };
        let strike = self.strikes[self.nearest(size)];
        let mut record = [0u8; RECORD];
        let at = strike.glyph_table + (index * RECORD) as u64;
        if !self.source.read_at(at, &mut record) {
            return Glyph::empty(0.0);
        }
        let advance = q6(i32_le(&record, 4)).max(0.0);
        let (width, height) = (u16_le(&record, 12), u16_le(&record, 14));
        let (w, h) = (usize::from(width), usize::from(height));
        if !glyph::fits(w, h, f32::from(strike.size) / 4.0) {
            return Glyph::empty(advance);
        }
        let Some(mut data) = zeroed(w.div_ceil(2) * h) else {
            return Glyph::empty(advance);
        };
        let start = strike.bitmaps + u64::from(u32_le(&record, 16));
        if !self.source.read_at(start, &mut data) {
            return Glyph::empty(advance);
        }
        Glyph {
            left: i16_le(&record, 8),
            top: i16_le(&record, 10),
            width,
            height,
            advance,
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::{BitmapFont, PackBuilder};
    use crate::glyph::pack_coverage;
    use core::cell::Cell;

    /// A pack in memory, counting the reads.
    struct Memory(Vec<u8>, Cell<usize>);

    impl PackSource for Memory {
        fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
            self.1.set(self.1.get() + 1);
            let at = at as usize;
            match self.0.get(at..at + buf.len()) {
                Some(bytes) => {
                    buf.copy_from_slice(bytes);
                    true
                }
                None => false,
            }
        }
    }

    fn glyph(width: u16, value: u8, advance: f32) -> Glyph {
        let coverage = alloc::vec![value; usize::from(width) * 2];
        Glyph {
            left: 1,
            top: -2,
            width,
            height: 2,
            advance,
            data: pack_coverage(&coverage, usize::from(width), 2).unwrap(),
        }
    }

    fn pack() -> Vec<u8> {
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 12.5,
        };
        let mut builder = PackBuilder::new();
        builder.add_strike(
            10.0,
            metrics,
            alloc::vec![('ж', glyph(3, 255, 4.0)), ('а', glyph(2, 136, 3.5))],
        );
        builder.add_strike(
            20.0,
            metrics,
            alloc::vec![('ж', glyph(6, 255, 8.0)), ('а', glyph(5, 255, 7.0))],
        );
        builder.build()
    }

    #[test]
    fn tables_are_where_the_directory_says() {
        let plain = StoredPack::open(Rc::new(Memory(pack(), Cell::new(0)))).unwrap();
        assert_eq!(plain.table(BitmapFont::ANCHORS), None);
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 12.5,
        };
        let mut builder = PackBuilder::new();
        builder.add_strike(10.0, metrics, alloc::vec![('а', glyph(2, 136, 3.5))]);
        builder.table(BitmapFont::ANCHORS, 1, &[0, 0, 0, 0]);
        let data = builder.build();
        let pack = StoredPack::open(Rc::new(Memory(data.clone(), Cell::new(0)))).unwrap();
        let at = pack.table(BitmapFont::ANCHORS).unwrap() as usize;
        assert_eq!(&data[at..at + 2], &[1, 0]);
        assert_eq!(pack.table(BitmapFont::SCRIPTS), None);
        assert_eq!(pack.table(8), None);
        assert_eq!(pack.advance('а', 10.0), 3.5);
    }

    #[test]
    fn reads_what_the_pack_in_memory_has() {
        let data = pack();
        let memory = Rc::new(Memory(data.clone(), Cell::new(0)));
        let stored = StoredPack::open(memory.clone()).unwrap();
        let in_place = BitmapFont::new(data).unwrap();
        for (ch, size) in [('а', 11.0), ('ж', 10.0), ('ж', 19.0)] {
            let a = stored.render(ch, size);
            let b = in_place.glyph(ch, size);
            assert_eq!(
                (a.left, a.top, a.width, a.height),
                (b.left, b.top, b.width, b.height)
            );
            assert_eq!(a.as_ref().data, b.data);
            assert_eq!(stored.advance(ch, size), b.advance);
        }
        assert!(stored.has_char('ж') && !stored.has_char('z'));
        assert!(stored.render('z', 10.0).width == 0);
        assert_eq!(stored.line_metrics(10.0).line_height, 12.5);
        // Advances are read once a strike.
        let reads = memory.1.get();
        stored.advance('а', 10.0);
        stored.advance('ж', 10.0);
        assert_eq!(memory.1.get(), reads);
    }

    #[test]
    fn negative_advances_read_as_zero() {
        let mut builder = PackBuilder::new();
        let metrics = LineMetrics::fallback(10.0);
        builder.add_strike(10.0, metrics, alloc::vec![('a', glyph(2, 255, -3.0))]);
        let stored = StoredPack::open(Rc::new(Memory(builder.build(), Cell::new(0)))).unwrap();
        assert_eq!(stored.advance('a', 10.0), 0.0);
        assert_eq!(stored.render('a', 10.0).advance, 0.0);
    }

    #[test]
    fn refuses_what_is_no_pack() {
        let memory = Rc::new(Memory(b"nope, no pack at all".to_vec(), Cell::new(0)));
        assert!(StoredPack::open(memory).is_err());
    }
}
