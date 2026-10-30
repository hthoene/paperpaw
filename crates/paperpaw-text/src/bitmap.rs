//! Pre-rendered bitmap font packs (`.ppfp`).
//!
//! Packs hold glyph bitmaps for a set of pixel sizes ("strikes"). They are
//! read in place (on devices that map flash into the address space they use
//! no RAM at all) and need no floating-point work at runtime.
//!
//! # Format (version 1, little endian)
//!
//! ```text
//! header   12 bytes  "PPFP", version u8, 3 reserved, strike count u16, reserved u16
//! strikes  32 bytes  size (1/4 px) u16, reserved u16, ascent, descent,
//!   each             line height (all i32, 1/64 px), glyph count u32,
//!                    glyph table offset u32, bitmap offset u32, reserved u32
//! glyphs   20 bytes  code point u32, advance i32 (1/64 px), left i16, top i16,
//!   each, sorted     width u16, height u16, data offset u32 (from bitmap offset)
//! bitmaps            4-bit coverage, two pixels per byte, rows padded
//! kerning            optional, see below
//! ```
//!
//! The first strike's last reserved field is the offset of the pack's
//! kerning table (0: none), which holds pairs of characters and how much
//! they move the second one, in classes (see `kern_table.rs`). Readers
//! made before it ignore the field, and packs made before it read
//! without kerning: the version stays 1.
//!
//! # Tables (version 2)
//!
//! A pack of version 2 is one of version 1 with a directory of eight
//! `u32` offsets of tables (0: absent) right after its strikes' headers.
//! A pack without tables is written as version 1, which every reader
//! reads. The slots ([`BitmapFont::ANCHORS`] and on):
//!
//! ```text
//! 0  anchors              where marks sit on letters (below)
//! 1  mark-to-mark anchors the same format, for a mark on a mark where
//!                         it differs from slot 0
//! 2  scripts              the scripts the pack is for and their line
//!                         metrics (below)
//! 3  language forms       per language, keys drawn as other keys
//! 4  substitutions        contextual substitutions
//! 5  vertical metrics     for vertical lines
//! 6  reserved             0
//! 7  next directory       eight more offsets, slots 8 to 15, the same way
//! ```
//!
//! Every table starts with a version byte, three reserved bytes and its
//! length in bytes (`u32`, these eight included); a reader skips a table
//! whose version it does not know. The formats of slots 3 to 5 are given
//! with their first version.
//!
//! Anchors (version 1), per strike, as packs are per size:
//!
//! ```text
//! head       8 bytes   version 1, 3 reserved, length u32
//! strikes    4 bytes   per strike of the pack: offset of its part u32
//!   each               (from the table's start)
//! part       4 bytes   key count u32, then per key, sorted by key:
//! keys      12 bytes   key u32, anchors' offset u32 (from the table's
//!   each               start), anchor count u8, attach class u8
//!                      (a mark's: the class of the anchor it sits on;
//!                      255 for a letter), 2 reserved
//! anchors    3 bytes   class u8, x i8, y i8 (pixels from the pen and
//!   each               the baseline, y down)
//! ```
//!
//! Anchor classes: 0 above a letter, 1 below a letter, 2 above a mark
//! (mark to mark), 3 below a mark, 4 above right, 5 below right; 6 and 7
//! are not used yet.
//!
//! Scripts (version 1):
//!
//! ```text
//! head       8 bytes   version 1, 3 reserved, length u32
//! count      4 bytes   script count u8, 3 reserved
//! scripts    4 bytes   script u8 (its index in paperpaw_lang::scripts(),
//!   each               from 1), 3 reserved, then per strike of the
//!                      pack: ascent i32, descent i32 (1/64 px)
//! ```

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

use crate::font::{FontError, LineMetrics};
use crate::glyph::{Glyph, GlyphRef};
use crate::kern_table;

const MAGIC: &[u8; 4] = b"PPFP";
/// Packs made before PaperPaw had its name (the same format): still read.
const OLD_MAGIC: &[u8; 4] = b"QIFP"; // before PPFP
const VERSION: u8 = 1;
/// A pack with a directory of tables.
const VERSION_TABLES: u8 = 2;
const HEADER: usize = 12;
/// The directory's slots, and its size.
const SLOTS: usize = 8;
const DIRECTORY: usize = SLOTS * 4;
/// A table's head: version, three reserved bytes, length.
const TABLE_HEAD: usize = 8;
const STRIKE: usize = 32;
const RECORD: usize = 20;
/// Where in the first strike's header the kerning table's offset is.
const KERNING: usize = HEADER + 28;

/// A bitmap font pack.
#[derive(Clone)]
pub struct BitmapFont {
    data: Cow<'static, [u8]>,
    strikes: u16,
}

impl fmt::Debug for BitmapFont {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BitmapFont")
            .field("bytes", &self.data.len())
            .field("strikes", &self.strikes)
            .finish()
    }
}

/// A view of one strike's header.
#[derive(Debug, Clone, Copy)]
struct Strike {
    size: f32,
    metrics: LineMetrics,
    glyphs: usize,
    glyph_table: usize,
    bitmaps: usize,
}

impl BitmapFont {
    pub fn is_pack(data: &[u8]) -> bool {
        data.starts_with(MAGIC) || data.starts_with(OLD_MAGIC)
    }

    /// Validates the pack's structure.
    pub fn new(data: impl Into<Cow<'static, [u8]>>) -> Result<BitmapFont, FontError> {
        let data = data.into();
        if !Self::is_pack(&data) || data.len() < HEADER {
            return Err(FontError::Invalid);
        }
        if data[4] != VERSION && data[4] != VERSION_TABLES {
            return Err(FontError::UnsupportedVersion);
        }
        let strikes = u16_at(&data, 8).ok_or(FontError::Invalid)?;
        let font = BitmapFont { data, strikes };
        if strikes == 0 {
            return Err(FontError::Invalid);
        }
        for i in 0..usize::from(strikes) {
            let strike = font.strike(i).ok_or(FontError::Invalid)?;
            // Checked: on 32-bit chips a large count would wrap.
            let end = strike
                .glyphs
                .checked_mul(RECORD)
                .and_then(|table| table.checked_add(strike.glyph_table));
            if end.is_none_or(|end| end > font.data.len()) || strike.bitmaps > font.data.len() {
                return Err(FontError::Invalid);
            }
        }
        Ok(font)
    }

    /// The slot of the anchors table (see the module's documentation).
    pub const ANCHORS: usize = 0;
    /// The slot of the mark-to-mark anchors table.
    pub const MARK_ANCHORS: usize = 1;
    /// The slot of the scripts table: scripts and their line metrics.
    pub const SCRIPTS: usize = 2;
    /// The slot of the language forms table.
    pub const LANGUAGE_FORMS: usize = 3;
    /// The slot of the contextual substitutions table.
    pub const SUBSTITUTIONS: usize = 4;
    /// The slot of the vertical metrics table.
    pub const VERTICAL: usize = 5;
    /// The slot of the next directory: slots 8 to 15.
    pub const NEXT: usize = 7;

    /// Table `slot` of the pack (see the module's documentation), from
    /// its head to its end, if it has one whole: read in place.
    pub fn table(&self, slot: usize) -> Option<&[u8]> {
        if self.data.get(4) != Some(&VERSION_TABLES) {
            return None;
        }
        let mut directory = HEADER.checked_add(usize::from(self.strikes) * STRIKE)?;
        let mut slot = slot;
        // Slots past 7 through the next directories, at most a few.
        for _ in 0..4 {
            if slot < Self::NEXT {
                let at = u32_at(&self.data, directory.checked_add(slot * 4)?)? as usize;
                return table_at(&self.data, at);
            }
            let next = u32_at(&self.data, directory.checked_add(Self::NEXT * 4)?)? as usize;
            if next == 0 || next.checked_add(DIRECTORY)? > self.data.len() {
                return None;
            }
            directory = next;
            slot -= SLOTS;
        }
        None
    }

    /// Available sizes in pixels.
    pub fn sizes(&self) -> impl Iterator<Item = f32> + '_ {
        (0..usize::from(self.strikes)).filter_map(|i| self.strike(i).map(|s| s.size))
    }

    paperpaw_table::shared! {
    /// The largest size text can be set in, in pixels: of the sizes with
    /// every character (not those made large for a few, a clock's).
    pub fn largest_size(&self) -> Option<f32> {
        let strikes: Vec<Strike> = (0..usize::from(self.strikes))
            .filter_map(|i| self.strike(i))
            .collect();
        let full = strikes.iter().map(|s| s.glyphs).max()?;
        strikes
            .iter()
            .filter(|s| s.glyphs >= full)
            .map(|s| s.size)
            .reduce(f32::max)
    } @ text::PACK_LARGEST_SIZE
    }

    fn strike(&self, index: usize) -> Option<Strike> {
        let base = HEADER + index * STRIKE;
        let d = &self.data;
        Some(Strike {
            size: f32::from(u16_at(d, base)?) / 4.0,
            metrics: LineMetrics {
                ascent: q6(i32_at(d, base + 4)?),
                descent: q6(i32_at(d, base + 8)?),
                line_height: q6(i32_at(d, base + 12)?),
            },
            glyphs: u32_at(d, base + 16)? as usize,
            glyph_table: u32_at(d, base + 20)? as usize,
            bitmaps: u32_at(d, base + 24)? as usize,
        })
    }

    /// The index of the strike closest to `size`, in whole numbers as the
    /// pack keeps sizes (quarter pixels): asked for every character drawn
    /// or measured, on chips that reckon in floats slowly.
    fn nearest_index(&self, size: f32) -> Option<usize> {
        self.nearest_quarters((size * 4.0) as i32)
    }

    /// The index of the strike closest to `wanted` quarter pixels; the
    /// first of those as close.
    fn nearest_quarters(&self, wanted: i32) -> Option<usize> {
        (0..usize::from(self.strikes))
            .filter_map(|i| Some((i, i32::from(u16_at(&self.data, HEADER + i * STRIKE)?))))
            .min_by_key(|&(_, quarters)| quarters.abs_diff(wanted))
            .map(|(i, _)| i)
    }

    /// The strike closest to `wanted` quarter pixels (as
    /// [`BitmapFont::nearest_quarters`]), its number of glyphs and the
    /// largest number any strike has, in one pass over the strikes.
    fn nearest_and_full(&self, wanted: i32) -> Option<(usize, usize, usize)> {
        let mut nearest: Option<(usize, u32, usize)> = None;
        let mut full = 0;
        for i in 0..usize::from(self.strikes) {
            let base = HEADER + i * STRIKE;
            // `new` read every strike's header: only the count is needed.
            let glyphs = u32_at(&self.data, base + 16).map_or(0, |n| n as usize);
            full = full.max(glyphs);
            let Some(quarters) = u16_at(&self.data, base) else {
                continue;
            };
            let distance = i32::from(quarters).abs_diff(wanted);
            if nearest.is_none_or(|(_, closest, _)| distance < closest) {
                nearest = Some((i, distance, glyphs));
            }
        }
        nearest.map(|(i, _, glyphs)| (i, glyphs, full))
    }

    /// The strike closest to `size`.
    fn nearest(&self, size: f32) -> Option<Strike> {
        self.strike(self.nearest_index(size)?)
    }

    paperpaw_table::shared! {
    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        self.nearest(size)
            .map_or(LineMetrics::fallback(size), |s| s.metrics)
    } @ text::PACK_LINE_METRICS
    }

    /// Where a strike's glyphs are: their number, their table and their
    /// bitmaps (what finding a glyph needs, without the metrics).
    fn tables(&self, index: usize) -> Option<Tables> {
        let base = HEADER + index * STRIKE;
        let d = &self.data;
        Some(Tables {
            glyphs: u32_at(d, base + 16)? as usize,
            glyph_table: u32_at(d, base + 20)? as usize,
            bitmaps: u32_at(d, base + 24)? as usize,
        })
    }

    fn record(&self, strike: &Tables, ch: char) -> Option<usize> {
        let target = ch as u32;
        // Most packs start with the printable ASCII characters, one after
        // the other: where the record at the character's place is its own,
        // it is the one the search finds, as codes are sorted and unique.
        if let Some(index) = target.checked_sub(0x20)
            && target < 0x7f
            && (index as usize) < strike.glyphs
        {
            let offset = strike.glyph_table + index as usize * RECORD;
            if u32_at(&self.data, offset) == Some(target) {
                return Some(offset);
            }
        }
        let (mut low, mut high) = (0usize, strike.glyphs);
        while low < high {
            let mid = (low + high) / 2;
            let offset = strike.glyph_table + mid * RECORD;
            let code = u32_at(&self.data, offset)?;
            match code.cmp(&target) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return Some(offset),
            }
        }
        None
    }

    paperpaw_table::shared! {
    pub fn has_char(&self, ch: char) -> bool {
        self.nearest_quarters(0)
            .and_then(|i| self.tables(i))
            .is_some_and(|strike| self.record(&strike, ch).is_some())
    } @ text::PACK_HAS_CHAR
    }

    paperpaw_table::shared! {
    /// The size `text` is drawn at when asked for `size`, at a strike
    /// that has every character of the pack (the sizes text is drawn at),
    /// or, for a `display` (a clock's time, a number, drawn large alone),
    /// also at a large strike made for a few characters where it has all
    /// of `text`'s: the nearest of those, one size for the whole text.
    pub fn size_for(&self, size: f32, text: &str, display: bool) -> f32 {
        let wanted = (size * 4.0) as i32;
        let Some((nearest, glyphs, full)) = self.nearest_and_full(wanted) else {
            return size;
        };
        // Most strikes have every character: nothing to look up.
        if glyphs >= full {
            return size;
        }
        let count = |i: usize| self.tables(i).map_or(0, |t| t.glyphs);
        let has_all = |i: usize| {
            display
                && self.tables(i).is_some_and(|tables| {
                    text.chars()
                        .all(|ch| ch == ' ' || self.record(&tables, ch).is_some())
                })
        };
        if has_all(nearest) {
            return size;
        }
        (0..usize::from(self.strikes))
            .filter(|&i| has_all(i) || count(i) >= full)
            .filter_map(|i| Some((i, i32::from(u16_at(&self.data, HEADER + i * STRIKE)?))))
            .min_by_key(|&(_, quarters)| quarters.abs_diff(wanted))
            .map_or(size, |(_, quarters)| quarters as f32 / 4.0)
    } @ text::PACK_SIZE_FOR
    }

    /// The strike nearest to `size` that has `ch`, and its record: for a
    /// character a large strike made for a few characters lacks (a
    /// clock's digits, drawn large alone).
    fn nearest_with(&self, size: f32, ch: char) -> Option<(Tables, usize)> {
        let wanted = (size * 4.0) as i32;
        (0..usize::from(self.strikes))
            .filter_map(|i| {
                let quarters = i32::from(u16_at(&self.data, HEADER + i * STRIKE)?);
                let tables = self.tables(i)?;
                let record = self.record(&tables, ch)?;
                Some((quarters.abs_diff(wanted), tables, record))
            })
            .min_by_key(|(distance, ..)| *distance)
            .map(|(_, tables, record)| (tables, record))
    }

    paperpaw_table::shared! {
    /// The glyph for `ch` at the strike nearest to `size`; where that
    /// strike lacks it (a large one made for digits), at the nearest that
    /// has it. Missing characters fall back to U+FFFD, then `?`.
    pub fn glyph(&self, ch: char, size: f32) -> GlyphRef<'_> {
        let empty = GlyphRef {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            advance: 0.0,
            data: &[],
        };
        let Some(nearest) = self.nearest_index(size).and_then(|i| self.tables(i)) else {
            return empty;
        };
        let found = match self.record(&nearest, ch) {
            Some(record) => Some((nearest, record)),
            None => self.nearest_with(size, ch).or_else(|| {
                let fallback = ['\u{fffd}', '?']
                    .into_iter()
                    .find_map(|c| self.nearest_with(size, c))?;
                Some(fallback)
            }),
        };
        let Some((strike, offset)) = found else {
            return empty;
        };
        let d = &self.data;
        let read = || -> Option<GlyphRef<'_>> {
            let width = u16_at(d, offset + 12)?;
            let height = u16_at(d, offset + 14)?;
            let start = strike
                .bitmaps
                .checked_add(u32_at(d, offset + 16)? as usize)?;
            let len = usize::from(width).div_ceil(2) * usize::from(height);
            Some(GlyphRef {
                advance: q6(i32_at(d, offset + 4)?).max(0.0),
                left: i16_at(d, offset + 8)?,
                top: i16_at(d, offset + 10)?,
                width,
                height,
                data: d.get(start..start.checked_add(len)?)?,
            })
        };
        read().unwrap_or(empty)
    } @ text::PACK_GLYPH
    }

    paperpaw_table::shared! {
    pub fn advance(&self, ch: char, size: f32) -> f32 {
        self.glyph(ch, size).advance
    } @ text::PACK_ADVANCE
    }

    paperpaw_table::shared! {
    /// How much the pair `left`, `right` moves the second character, in
    /// pixels at the strike nearest to `size` (negative: closer); 0 for a
    /// pack without kerning. Read in place: no memory.
    pub fn kern(&self, left: char, right: char, size: f32) -> f32 {
        let table = u32_at(&self.data, KERNING).unwrap_or(0) as usize;
        if table == 0 {
            return 0.0;
        }
        let units = kern_table::lookup(&self.data, table, left, right);
        if units == 0 {
            return 0.0;
        }
        let Some(quarters) = self
            .nearest_index(size)
            .and_then(|i| u16_at(&self.data, HEADER + i * STRIKE))
        else {
            return 0.0;
        };
        f32::from(units) * f32::from(quarters) / (4.0 * kern_table::UNITS)
    } @ text::PACK_KERN
    }
}

/// Where a strike's glyphs are (see [`BitmapFont::tables`]).
struct Tables {
    glyphs: usize,
    glyph_table: usize,
    bitmaps: usize,
}

/// Builds a pack from rasterized glyphs (used by `paperpaw-fontgen`).
#[derive(Debug, Default)]
pub struct PackBuilder {
    strikes: Vec<StrikeData>,
    /// The kerning table, if any (see [`PackBuilder::kerning`]).
    kerning: Vec<u8>,
    /// The tables of slots 0 to 6, with their heads (see
    /// [`PackBuilder::table`]).
    tables: [Vec<u8>; 7],
}

#[derive(Debug)]
struct StrikeData {
    size: f32,
    metrics: LineMetrics,
    glyphs: Vec<(char, Glyph)>,
}

impl PackBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a strike; glyphs may come in any order.
    pub fn add_strike(&mut self, size: f32, metrics: LineMetrics, mut glyphs: Vec<(char, Glyph)>) {
        glyphs.sort_by_key(|(ch, _)| *ch);
        glyphs.dedup_by_key(|(ch, _)| *ch);
        self.strikes.push(StrikeData {
            size,
            metrics,
            glyphs,
        });
    }

    /// Kerns the pack's text with `pairs`: two characters each and how
    /// much the second moves, in ems (negative: closer), as
    /// [`OutlineFont::kerning_pairs`](crate::OutlineFont::kerning_pairs)
    /// finds them. Kept in 1/1024 em; characters from U+10000 on are not
    /// kerned.
    pub fn kerning(&mut self, pairs: &[(char, char, f32)]) {
        self.kerning = kern_table::build(pairs);
    }

    /// Puts `body` in slot `slot` (0 to 6, see [`BitmapFont::ANCHORS`])
    /// as a table of version `version`, its head before it; an empty body
    /// takes it out. A pack with a table is of version 2.
    pub fn table(&mut self, slot: usize, version: u8, body: &[u8]) {
        let Some(table) = self.tables.get_mut(slot) else {
            return;
        };
        table.clear();
        if body.is_empty() {
            return;
        }
        let len = (TABLE_HEAD + body.len()) as u32;
        table.extend_from_slice(&[version, 0, 0, 0]);
        table.extend_from_slice(&len.to_le_bytes());
        table.extend_from_slice(body);
    }

    pub fn build(&self) -> Vec<u8> {
        let tables = self.tables.iter().any(|t| !t.is_empty());
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        let version = if tables { VERSION_TABLES } else { VERSION };
        out.extend_from_slice(&[version, 0, 0, 0]);
        out.extend_from_slice(&(self.strikes.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);

        let strike_table = out.len();
        out.resize(strike_table + self.strikes.len() * STRIKE, 0);
        let directory = out.len();
        if tables {
            out.resize(directory + DIRECTORY, 0);
        }
        for (index, strike) in self.strikes.iter().enumerate() {
            let (size, metrics, glyphs) = (strike.size, strike.metrics, &strike.glyphs);
            let glyph_table = out.len();
            out.resize(glyph_table + glyphs.len() * RECORD, 0);
            let bitmaps = out.len();
            for (i, (ch, glyph)) in glyphs.iter().enumerate() {
                let data_offset = (out.len() - bitmaps) as u32;
                out.extend_from_slice(&glyph.data);
                let mut record = Vec::with_capacity(RECORD);
                record.extend_from_slice(&(*ch as u32).to_le_bytes());
                record.extend_from_slice(&to_q6(glyph.advance).to_le_bytes());
                record.extend_from_slice(&glyph.left.to_le_bytes());
                record.extend_from_slice(&glyph.top.to_le_bytes());
                record.extend_from_slice(&glyph.width.to_le_bytes());
                record.extend_from_slice(&glyph.height.to_le_bytes());
                record.extend_from_slice(&data_offset.to_le_bytes());
                let at = glyph_table + i * RECORD;
                out[at..at + RECORD].copy_from_slice(&record);
            }
            let mut header = Vec::with_capacity(STRIKE);
            header.extend_from_slice(&((size * 4.0 + 0.5) as u16).to_le_bytes());
            header.extend_from_slice(&[0, 0]);
            header.extend_from_slice(&to_q6(metrics.ascent).to_le_bytes());
            header.extend_from_slice(&to_q6(metrics.descent).to_le_bytes());
            header.extend_from_slice(&to_q6(metrics.line_height).to_le_bytes());
            header.extend_from_slice(&(glyphs.len() as u32).to_le_bytes());
            header.extend_from_slice(&(glyph_table as u32).to_le_bytes());
            header.extend_from_slice(&(bitmaps as u32).to_le_bytes());
            header.extend_from_slice(&[0, 0, 0, 0]);
            let at = strike_table + index * STRIKE;
            out[at..at + STRIKE].copy_from_slice(&header);
        }
        if !self.kerning.is_empty() && !self.strikes.is_empty() {
            let table = (out.len() as u32).to_le_bytes();
            out[KERNING..KERNING + 4].copy_from_slice(&table);
            out.extend_from_slice(&self.kerning);
        }
        for (slot, table) in self.tables.iter().enumerate() {
            if !table.is_empty() {
                let at = directory + slot * 4;
                let offset = (out.len() as u32).to_le_bytes();
                out[at..at + 4].copy_from_slice(&offset);
                out.extend_from_slice(table);
            }
        }
        out
    }
}

fn q6(value: i32) -> f32 {
    value as f32 / 64.0
}

fn to_q6(value: f32) -> i32 {
    let scaled = value * 64.0;
    if scaled < 0.0 {
        (scaled - 0.5) as i32
    } else {
        (scaled + 0.5) as i32
    }
}

fn bytes<const N: usize>(data: &[u8], at: usize) -> Option<[u8; N]> {
    data.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// The table at `at` of `data`, if it fits whole.
fn table_at(data: &[u8], at: usize) -> Option<&[u8]> {
    if at == 0 {
        return None;
    }
    let len = u32_at(data, at.checked_add(4)?)? as usize;
    if len < TABLE_HEAD {
        return None;
    }
    data.get(at..at.checked_add(len)?)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    bytes(data, at).map(u16::from_le_bytes)
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    bytes(data, at).map(i16::from_le_bytes)
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    bytes(data, at).map(u32::from_le_bytes)
}

fn i32_at(data: &[u8], at: usize) -> Option<i32> {
    bytes(data, at).map(i32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph::pack_coverage;

    /// A pack with every character at 10 and 20 px, and digits alone at a
    /// large 80 px, as the UI font's are made for a clock.
    fn display_pack() -> BitmapFont {
        let metrics = |size: f32| LineMetrics {
            ascent: size * 0.8,
            descent: size * 0.2,
            line_height: size * 1.25,
        };
        let mut builder = PackBuilder::new();
        for size in [10.0, 20.0] {
            let glyphs = "12:Ab"
                .chars()
                .map(|c| (c, glyph(2, 255, size / 2.0)))
                .collect();
            builder.add_strike(size, metrics(size), glyphs);
        }
        let digits = "12:".chars().map(|c| (c, glyph(2, 255, 40.0))).collect();
        builder.add_strike(80.0, metrics(80.0), digits);
        BitmapFont::new(builder.build()).unwrap()
    }

    #[test]
    fn large_sizes_for_a_few_characters_draw_a_display_of_those_alone() {
        let font = display_pack();
        // Text at the sizes with every character, as before.
        assert_eq!(font.size_for(80.0, "12:21", false), 20.0);
        assert_eq!(font.size_for(80.0, "Ab", false), 20.0);
        assert_eq!(font.size_for(20.0, "12", false), 20.0);
        // A display of digits at the large size, one with letters not.
        assert_eq!(font.size_for(80.0, "12:21", true), 80.0);
        assert_eq!(font.size_for(70.0, "1 2", true), 70.0);
        assert_eq!(font.size_for(80.0, "12 Ab", true), 20.0);
        // A character a large strike lacks comes from the nearest that has
        // it, not as a question mark.
        assert_eq!(font.glyph('A', 80.0).advance, 10.0);
        assert_eq!(font.glyph('1', 80.0).advance, 40.0);
        assert_eq!(font.glyph('€', 80.0).width, 0);
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

    fn pack() -> BitmapFont {
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 12.5,
        };
        let mut builder = PackBuilder::new();
        builder.add_strike(
            10.0,
            metrics,
            alloc::vec![('b', glyph(3, 255, 4.0)), ('a', glyph(2, 136, 3.5))],
        );
        builder.add_strike(20.0, metrics, alloc::vec![('a', glyph(5, 255, 7.0))]);
        BitmapFont::new(builder.build()).unwrap()
    }

    #[test]
    fn round_trips_glyphs_and_metrics() {
        let font = pack();
        assert_eq!(font.sizes().collect::<Vec<_>>(), [10.0, 20.0]);
        let a = font.glyph('a', 11.0);
        assert_eq!((a.width, a.height, a.left, a.top), (2, 2, 1, -2));
        assert_eq!(a.advance, 3.5);
        assert_eq!(a.coverage(1, 1), 136);
        assert_eq!(font.glyph('a', 18.0).width, 5);
        assert_eq!(font.line_metrics(10.0).line_height, 12.5);
        assert!(font.has_char('b'));
    }

    #[test]
    fn records_are_found_with_and_without_all_of_ascii() {
        let mut builder = PackBuilder::new();
        let metrics = LineMetrics::fallback(10.0);
        let strike = |chars: &mut dyn Iterator<Item = char>| {
            chars.map(|c| (c, glyph(1, 255, 1.0))).collect::<Vec<_>>()
        };
        // All of printable ASCII, the same with gaps, and a few letters.
        builder.add_strike(10.0, metrics, strike(&mut (' '..='~').chain(['é', 'Ж'])));
        let gaps = ('!'..='~').filter(|c| !('A'..='F').contains(c));
        builder.add_strike(20.0, metrics, strike(&mut gaps.chain(['é'])));
        builder.add_strike(30.0, metrics, strike(&mut "xyz".chars()));
        let font = BitmapFont::new(builder.build()).unwrap();
        for i in 0..3 {
            let tables = font.tables(i).unwrap();
            let scanned = |ch: char| {
                (0..tables.glyphs)
                    .map(|n| tables.glyph_table + n * RECORD)
                    .find(|&at| u32_at(&font.data, at) == Some(ch as u32))
            };
            for ch in ('\0'..='\u{17f}').chain(['Ж', char::MAX]) {
                assert_eq!(font.record(&tables, ch), scanned(ch), "{i} {ch:?}");
            }
        }
    }

    #[test]
    fn missing_characters_fall_back() {
        let font = pack();
        assert!(font.glyph('z', 10.0).is_empty());
        assert_eq!(font.advance('z', 10.0), 0.0);
    }

    #[test]
    fn tables_are_found_in_their_slots() {
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 10.0,
        };
        let mut builder = PackBuilder::new();
        builder.add_strike(10.0, metrics, alloc::vec![('a', glyph(2, 136, 3.5))]);
        let plain = builder.build();
        assert_eq!(plain[4], VERSION);
        builder.table(BitmapFont::SCRIPTS, 1, &[1, 0, 0, 0, 7]);
        builder.table(9, 1, &[1]);
        let font = BitmapFont::new(builder.build()).unwrap();
        assert_eq!(font.data[4], VERSION_TABLES);
        let table = font.table(BitmapFont::SCRIPTS).unwrap();
        assert_eq!(table.len(), TABLE_HEAD + 5);
        assert_eq!((table[0], table[TABLE_HEAD + 4]), (1, 7));
        for slot in [0, 1, 6, 7, 8, 15, 99] {
            assert!(font.table(slot).is_none());
        }
        // The glyphs read as before; a pack of version 1 has no tables.
        assert_eq!(font.glyph('a', 10.0).advance, 3.5);
        assert!(
            BitmapFont::new(plain)
                .unwrap()
                .table(BitmapFont::SCRIPTS)
                .is_none()
        );
        // A table that says it is longer than the pack is not read.
        let mut data = font.data.into_owned();
        let len = data.len();
        data[len - 9] = 0xFF;
        assert!(
            BitmapFont::new(data)
                .unwrap()
                .table(BitmapFont::SCRIPTS)
                .is_none()
        );
    }

    #[test]
    fn rejects_corrupt_packs() {
        let mut data = pack().data.into_owned();
        assert!(BitmapFont::new(data[..20].to_vec()).is_err());
        data[4] = 9;
        assert_eq!(
            BitmapFont::new(data).unwrap_err(),
            FontError::UnsupportedVersion
        );
        assert!(BitmapFont::new(b"nope".to_vec()).is_err());
    }

    #[test]
    fn negative_advances_read_as_zero() {
        let mut builder = PackBuilder::new();
        let metrics = LineMetrics::fallback(10.0);
        builder.add_strike(10.0, metrics, alloc::vec![('a', glyph(2, 255, -3.0))]);
        let font = BitmapFont::new(builder.build()).unwrap();
        assert_eq!(font.advance('a', 10.0), 0.0);
    }

    #[test]
    fn reads_past_the_address_space_find_nothing() {
        let data = [0u8; 8];
        assert_eq!(u32_at(&data, usize::MAX - 1), None);
        assert_eq!(u16_at(&data, usize::MAX), None);
    }

    #[test]
    fn reads_packs_made_under_the_old_name() {
        let mut data = pack().data.into_owned();
        data[..4].copy_from_slice(OLD_MAGIC);
        assert!(BitmapFont::new(data).unwrap().has_char('b'));
    }
}

#[cfg(test)]
mod format_tests;
