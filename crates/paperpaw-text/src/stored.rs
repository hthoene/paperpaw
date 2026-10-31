//! Font packs read where they are kept (e.g. on an SD card), a glyph at a
//! time, for scripts a device's built-in packs lack: only the characters
//! a pack holds and, for each size in use, their advances stay in memory,
//! compactly (see [`index`]: some bytes for a block held whole, a bit a
//! code point where it has gaps, runs of the same advance); glyphs are
//! read when first drawn and kept in the [`GlyphCache`](crate::GlyphCache)
//! like rasterized ones.
//!
//! The format is the packs' (see [`BitmapFont`](crate::BitmapFont));
//! every strike must hold the same characters, as `paperpaw-fontgen`
//! makes them. What a damaged pack says is checked before memory is set
//! aside for it: a glyph larger than its size allows reads as empty.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::fmt;
use core::mem::size_of;

use crate::anchors::{self, Anchors};
use crate::bitmap::BitmapFont;
use crate::font::{FontError, LineMetrics};
use crate::glyph::{self, Glyph, zeroed};
use paperpaw_platform::MemoryClass;

mod index;

use index::{Advances, AdvancesBuilder, CharIndex, IndexBuilder, Refused};

const MAGIC: &[u8; 4] = b"PPFP";
/// Packs made before PaperPaw had its name (the same format): still read.
const OLD_MAGIC: &[u8; 4] = b"QIFP"; // before PPFP
const VERSION: u8 = 1;
/// A pack with a directory of tables (see [`StoredPack::table`]).
const VERSION_TABLES: u8 = 2;
const HEADER: usize = 12;
const STRIKE: usize = 32;
const RECORD: usize = 20;
/// Characters a pack may hold at most (a script or a few: all of
/// Hangul's syllables, or thousands of CJK ideographs).
const MAX_CHARS: usize = 65_535;
/// Glyph records read at once while a pack's characters or a strike's
/// advances are gathered (8,000 bytes).
const BLOCK: usize = 400;
/// Sizes a pack may hold at most.
const MAX_STRIKES: usize = 64;

/// Where a pack's bytes come from.
pub trait PackSource {
    /// Fills `buf` from `at`; `false` if that could not be read whole.
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool;
}

/// A table of a pack, read through its source from its offset.
struct Table<'a>(&'a dyn PackSource, u64);

impl anchors::Bytes for Table<'_> {
    fn bytes(&self, at: usize, buf: &mut [u8]) -> bool {
        u64::try_from(at)
            .ok()
            .and_then(|at| self.1.checked_add(at))
            .is_some_and(|at| self.0.read_at(at, buf))
    }
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

/// What the text family reads of a pack: general reading primitives,
/// behind a trait object so that how a pack keeps its characters,
/// advances and tables in memory can change in any later firmware without
/// changing the plugin interface (which pins these items, not the type
/// behind them). Later tables (per-script metrics, language forms,
/// substitutions, vertical metrics) are read through [`Pack::table`] and
/// [`Pack::source`]: no new item.
///
/// Items are only ever appended, each with a higher [`Pack::revision`]
/// than the last: code calls an item that came with revision `n` only
/// where the pack's revision is `n` or more (packs are made by the
/// firmware, or by a plugin's own copy of [`StoredPack::open`], either
/// older or newer than the code calling them). No supertraits, generic
/// or `Self`-returning items: its table's order is its items' (ADR 0026).
trait Pack {
    /// The items it has: those of this revision and before
    /// ([`PACK_REVISION`] here).
    fn revision(&self) -> u16;
    /// How many sizes (strikes) it has; at least one.
    fn strikes(&self) -> usize;
    /// The size of `strike`, in pixels.
    fn size(&self, strike: usize) -> f32;
    fn metrics(&self, strike: usize) -> LineMetrics;
    /// The strike to draw `size` with: the nearest; of two as close, the
    /// smaller.
    fn strike(&self, size: f32) -> usize;
    /// How many characters it holds, in all strikes together.
    fn chars(&self) -> usize;
    /// The place of `ch` among them, if it holds it: the same in every
    /// strike.
    fn index(&self, ch: char) -> Option<u32>;
    /// Whether `strike` draws the character at `index` (a pack may hold
    /// fewer characters in some sizes, e.g. CJK display sizes).
    fn covers(&self, strike: usize, index: u32) -> bool;
    /// The advance of the character at `index` in `strike`: 0 if it is
    /// not there, `None` if it could not be read now (a card pulled,
    /// memory short), which a layout is not to keep (see
    /// [`Pack::failed_reads`]).
    fn advance(&self, strike: usize, index: u32) -> Option<f32>;
    /// The glyph of the character at `index` in `strike`: empty if it is
    /// not there, `None` if it could not be read now (a card pulled,
    /// memory short), which is not to be kept as its glyph.
    fn render(&self, strike: usize, index: u32) -> Option<Glyph>;
    /// Where table `slot` starts in the pack, if it has one. Slot 7 is
    /// the next directory, of slots 8 to 15, whose last chains on as slot
    /// 7 does: tables have no fixed end.
    fn table(&self, slot: usize) -> Option<u64>;
    /// Where its bytes come from, to read its tables.
    fn source(&self) -> &dyn PackSource;
    /// The bytes of memory it holds.
    fn bytes(&self) -> usize;
    /// Lets go of what it reads again when needed (advances, blocks of
    /// records), for when memory is short.
    fn trim(&self);
    /// How many times it could not read an advance or a glyph it was
    /// asked for (a card pulled, memory short), wrapping: a layout that
    /// keeps pages (a book's page index) takes it before laying them out
    /// and keeps none if it changed, as their letters may have been
    /// measured 0 wide.
    fn failed_reads(&self) -> u32;
}

/// The revision of [`Pack`] that this code has.
const PACK_REVISION: u16 = 1;

/// What a pack read from a source may take of memory: chosen for the
/// device (its profile's [`MemoryClass`]), not the processor, so that an
/// emulator of a small device refuses what the device refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackLimits {
    /// The most bytes its index of characters may take (KS X 1001's
    /// Hangul take about 1.75 KiB, GB 2312's or Big5's ideographs about
    /// 3.3 KiB): a pack that would need more is refused
    /// ([`FontError::TooLarge`]).
    index: usize,
    /// The most bytes a strike's advances may take kept whole (4 KiB:
    /// 2,048 different ones, or as many runs of the same one); beyond
    /// that they are read when needed, [`ON_DEMAND`] at a time, as many
    /// kept as half of it holds.
    advances: usize,
}

impl PackLimits {
    /// For devices of a few hundred kilobytes (the Xteink X4's 256 KiB
    /// heap): an index of 8 KiB, advances of 4 KiB a size.
    pub const SMALL: PackLimits = PackLimits {
        index: 8 * 1024,
        advances: 4 * 1024,
    };
    /// For devices of megabytes (PSRAM, phones, computers): 32 KiB and
    /// 16 KiB.
    pub const LARGE: PackLimits = PackLimits {
        index: 32 * 1024,
        advances: 16 * 1024,
    };

    /// The limits of a device of `memory`.
    pub const fn for_memory(memory: MemoryClass) -> PackLimits {
        match memory {
            MemoryClass::Large => PackLimits::LARGE,
            _ => PackLimits::SMALL,
        }
    }

    /// The most bytes a pack's index of characters may take.
    pub const fn index_bytes(self) -> usize {
        self.index
    }

    /// The most bytes a size's advances may take kept whole.
    pub const fn advances_bytes(self) -> usize {
        self.advances
    }
}

/// Glyph records read at once for the advances of a strike that has too
/// many to keep whole (1,280 bytes): small, so that a page of several
/// scripts (Latin, Greek, Cyrillic and punctuation sit in blocks far
/// apart) finds all its blocks kept.
const ON_DEMAND: usize = 64;

/// A font pack read from a [`PackSource`]: a handle, cheap to clone.
#[derive(Clone)]
pub struct StoredPack(Rc<dyn Pack>);

impl fmt::Debug for StoredPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredPack")
            .field("revision", &self.0.revision())
            .field("strikes", &self.0.strikes())
            .field("chars", &self.0.chars())
            .field("bytes", &self.0.bytes())
            .finish()
    }
}

/// A pack as this firmware keeps it in memory: its characters and, for
/// each size in use, their advances (see [`index`]).
struct Paged {
    source: Rc<dyn PackSource>,
    strikes: Vec<Strike>,
    /// The characters it holds (the same in every strike).
    chars: CharIndex,
    /// Each strike's advances, once it was used.
    advances: RefCell<Vec<Kept>>,
    /// The offsets of its tables, 0 where absent.
    tables: [u32; 8],
    /// What it may keep of a strike's advances whole.
    advances_budget: usize,
    /// See [`Pack::failed_reads`].
    failed: Cell<u32>,
}

/// What a [`Paged`] keeps of a strike's advances, in 1/16 px.
#[derive(Debug, Default)]
enum Kept {
    /// Nothing yet.
    #[default]
    None,
    /// All of them.
    Whole(Advances),
    /// More than its budget would hold: the blocks of
    /// [`ON_DEMAND`] read last (the block's number and its advances),
    /// the latest last.
    Blocks(Vec<(u32, Vec<u16>)>),
}

impl Kept {
    fn bytes(&self) -> usize {
        match self {
            Kept::None => 0,
            Kept::Whole(advances) => advances.bytes(),
            Kept::Blocks(blocks) => {
                blocks.capacity() * size_of::<(u32, Vec<u16>)>()
                    + blocks
                        .iter()
                        .map(|(_, b)| b.capacity() * size_of::<u16>())
                        .sum::<usize>()
            }
        }
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

/// A record's advance in 1/16 px, 0 if negative.
fn advance_16(record: &[u8; RECORD]) -> u16 {
    (i32_le(record, 4) / 4).clamp(0, i32::from(u16::MAX)) as u16
}

/// Why [`read_records`] stopped before the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Halt {
    /// The source could not give them.
    Unread,
    /// No memory for a block of them.
    Memory,
    /// `each` said to stop.
    Stopped,
}

/// Reads the `n` glyph records at `at` [`BLOCK`] at a time, handing each
/// to `each` until it says to stop.
fn read_records(
    source: &dyn PackSource,
    at: u64,
    n: usize,
    mut each: impl FnMut(&[u8; RECORD]) -> bool,
) -> Result<(), Halt> {
    let mut block = zeroed(n.min(BLOCK) * RECORD).ok_or(Halt::Memory)?;
    let mut done = 0;
    while done < n {
        let count = (n - done).min(BLOCK);
        let bytes = &mut block[..count * RECORD];
        if !source.read_at(at + (done * RECORD) as u64, bytes) {
            return Err(Halt::Unread);
        }
        if !bytes.as_chunks::<RECORD>().0.iter().all(&mut each) {
            return Err(Halt::Stopped);
        }
        done += count;
    }
    Ok(())
}

impl StoredPack {
    /// Reads the pack's header and its characters, within the limits of
    /// the smallest devices ([`PackLimits::SMALL`]); see
    /// [`StoredPack::open_within`].
    pub fn open(source: Rc<dyn PackSource>) -> Result<StoredPack, FontError> {
        StoredPack::open_within(source, PackLimits::SMALL)
    }

    paperpaw_table::shared! {
    /// Reads the pack's header and its characters, within `limits` (the
    /// device's: [`PackLimits::for_memory`]): the firmware's code, so
    /// that how it keeps them is the firmware's too.
    /// [`FontError::OutOfMemory`] may pass: open it again later.
    pub fn open_within(
        source: Rc<dyn PackSource>,
        limits: PackLimits,
    ) -> Result<StoredPack, FontError> {
        Paged::open(source, limits).map(|pack| StoredPack(Rc::new(pack)))
    } @ text::STORED_OPEN
    }

    /// Where table `slot` (0 to 7, see
    /// [`BitmapFont::ANCHORS`](crate::BitmapFont::ANCHORS)) starts in the
    /// pack, if it has one: read it through its [`PackSource`]. Slot 7
    /// is the next directory, of slots 8 to 15.
    pub fn table(&self, slot: usize) -> Option<u64> {
        self.0.table(slot)
    }

    /// The largest size it has, in pixels.
    pub fn largest_size(&self) -> Option<f32> {
        (0..self.0.strikes())
            .map(|s| self.0.size(s))
            .reduce(f32::max)
    }

    /// Whether it draws `ch` in any of its sizes.
    pub fn has_char(&self, ch: char) -> bool {
        self.0
            .index(ch)
            .is_some_and(|index| (0..self.0.strikes()).any(|s| self.0.covers(s, index)))
    }

    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        self.0.metrics(self.0.strike(size))
    }

    /// The advance of `ch` at the strike nearest to `size`; 0 if it is
    /// not in the pack, or could not be read now (counted in
    /// [`StoredPack::failed_reads`]).
    pub fn advance(&self, ch: char, size: f32) -> f32 {
        self.0
            .index(ch)
            .and_then(|index| self.0.advance(self.0.strike(size), index))
            .unwrap_or(0.0)
    }

    /// Where marks sit on `ch` at the strike nearest to `size` (see
    /// [`crate::anchors`]): a few reads from the source.
    pub(crate) fn anchors(&self, ch: char, size: f32) -> Anchors {
        let (Some(table), Some(index)) = (self.0.table(BitmapFont::ANCHORS), self.0.index(ch))
        else {
            return Anchors::NONE;
        };
        let strike = self.0.strike(size);
        anchors::find(&Table(self.0.source(), table), strike, ch, index as usize)
    }

    /// The glyph of `ch` at the strike nearest to `size`, read from the
    /// source: empty if it is not in the pack, `None` if it could not be
    /// read now (the card pulled, memory short).
    pub fn render(&self, ch: char, size: f32) -> Option<Glyph> {
        match self.0.index(ch) {
            Some(index) => self.0.render(self.0.strike(size), index),
            None => Some(Glyph::empty(0.0)),
        }
    }

    /// The bytes of memory it holds.
    pub fn bytes(&self) -> usize {
        self.0.bytes()
    }

    /// Lets go of what it reads again when needed (the advances of its
    /// sizes), for when memory is short: drawing goes on, slower until
    /// they are read again.
    pub fn trim(&self) {
        self.0.trim();
    }

    /// How many times it could not read an advance or a glyph (a card
    /// pulled, memory short), wrapping: a layout that keeps pages takes
    /// it before laying them out and keeps none if it changed.
    pub fn failed_reads(&self) -> u32 {
        self.0.failed_reads()
    }
}

impl Paged {
    fn open(source: Rc<dyn PackSource>, limits: PackLimits) -> Result<Paged, FontError> {
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
        let mut table = zeroed::<u8>(count * STRIKE).ok_or(FontError::OutOfMemory)?;
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
        // In order, each once, in the index's budget.
        let mut chars = IndexBuilder::new(limits.index);
        let mut refused = None;
        let read = read_records(&*source, strikes[0].glyph_table, n, |r| {
            let pushed = match char::from_u32(u32_le(r, 0)) {
                Some(ch) => chars.push(ch),
                None => Err(Refused::Order),
            };
            refused = pushed.err();
            refused.is_none()
        });
        match (refused, read) {
            (Some(Refused::Budget), _) => return Err(FontError::TooLarge),
            (Some(Refused::Memory), _) | (_, Err(Halt::Memory)) => {
                return Err(FontError::OutOfMemory);
            }
            (Some(Refused::Order), _) | (_, Err(Halt::Unread | Halt::Stopped)) => {
                return Err(FontError::Invalid);
            }
            (None, Ok(())) => {}
        }
        let chars = chars.finish();
        if chars.len() != n {
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
        let mut advances = Vec::new();
        advances.resize_with(strikes.len(), Kept::default);
        Ok(Paged {
            source,
            advances: RefCell::new(advances),
            strikes,
            chars,
            tables,
            advances_budget: limits.advances,
            failed: Cell::new(0),
        })
    }

    /// The advances of `strike` (whose glyph table is at `glyph_table`),
    /// all of them if they fit its budget, else [`Kept::Blocks`], also
    /// where memory ran out (blocks need little; [`Pack::trim`] tries whole
    /// again): `None` if they could not be read, to try again.
    fn gather(&self, glyph_table: u64) -> Option<Kept> {
        let n = self.chars.len();
        let mut built = AdvancesBuilder::new(n, self.advances_budget);
        let mut refused = None;
        let read = read_records(&*self.source, glyph_table, n, |r| {
            refused = built.push(advance_16(r)).err();
            refused.is_none()
        });
        match (refused, read) {
            (Some(_), _) | (_, Err(Halt::Memory)) => Some(Kept::Blocks(Vec::new())),
            (None, Err(_)) => None,
            (None, Ok(())) => Some(Kept::Whole(built.finish())),
        }
    }

    /// The advance at `index` (below the pack's characters) of `strike`,
    /// in 1/16 px: `None` if it could not be read.
    fn read_advance(&self, strike: usize, glyph_table: u64, index: usize) -> Option<u16> {
        // Busy only if reading the source came back here.
        let mut advances = self.advances.try_borrow_mut().ok()?;
        let kept = advances.get_mut(strike)?;
        if let Kept::None = kept {
            // Read once; tried again where that failed.
            *kept = self.gather(glyph_table)?;
        }
        match kept {
            Kept::None => None,
            Kept::Whole(advances) => advances.get(index),
            Kept::Blocks(blocks) => self.block_advance(blocks, glyph_table, index),
        }
    }

    /// The advance at `index` of a strike whose advances are read in
    /// blocks, reading its block if it is not among `blocks`: `None` if
    /// it could not be read.
    fn block_advance(
        &self,
        blocks: &mut Vec<(u32, Vec<u16>)>,
        glyph_table: u64,
        index: usize,
    ) -> Option<u16> {
        let block = u32::try_from(index / ON_DEMAND).ok()?;
        if let Some(at) = blocks.iter().position(|(b, _)| *b == block) {
            // The latest last.
            let kept = blocks.remove(at);
            let advance = kept.1.get(index % ON_DEMAND).copied();
            blocks.push(kept);
            return advance;
        }
        let first = block as usize * ON_DEMAND;
        let count = self.chars.len().checked_sub(first)?.min(ON_DEMAND);
        let mut advances = Vec::new();
        advances.try_reserve_exact(count).ok()?;
        let at = glyph_table + (first * RECORD) as u64;
        read_records(&*self.source, at, count, |r| {
            advances.push(advance_16(r));
            true
        })
        .ok()?;
        let advance = advances.get(index % ON_DEMAND).copied();
        let kept = self.kept_blocks();
        if blocks.len() >= kept {
            blocks.remove(0);
        }
        if blocks.try_reserve_exact(kept - blocks.len()).is_ok() {
            blocks.push((block, advances));
        }
        advance
    }

    /// How many blocks of advances read when needed a strike keeps: as
    /// many as half its budget holds (16 on the ESP32-C3), two at least.
    fn kept_blocks(&self) -> usize {
        (self.advances_budget / 2 / (ON_DEMAND * size_of::<u16>())).max(2)
    }

    /// The glyph of the character at `index` in `strike`, as
    /// [`Pack::render`] says.
    fn read_glyph(&self, strike: Strike, index: u32) -> Option<Glyph> {
        let mut record = [0u8; RECORD];
        let at = strike.glyph_table + u64::from(index) * RECORD as u64;
        if !self.source.read_at(at, &mut record) {
            return None;
        }
        let advance = q6(i32_le(&record, 4)).max(0.0);
        let (width, height) = (u16_le(&record, 12), u16_le(&record, 14));
        let (w, h) = (usize::from(width), usize::from(height));
        if !glyph::fits(w, h, f32::from(strike.size) / 4.0) {
            // Damaged: drawn as nothing, every time.
            return Some(Glyph::empty(advance));
        }
        let mut data = zeroed(w.div_ceil(2) * h)?;
        let start = strike.bitmaps + u64::from(u32_le(&record, 16));
        if !self.source.read_at(start, &mut data) {
            return None;
        }
        Some(Glyph {
            left: i16_le(&record, 8),
            top: i16_le(&record, 10),
            width,
            height,
            advance,
            data,
        })
    }
}

impl Pack for Paged {
    fn revision(&self) -> u16 {
        PACK_REVISION
    }

    fn strikes(&self) -> usize {
        self.strikes.len()
    }

    fn size(&self, strike: usize) -> f32 {
        self.strikes
            .get(strike)
            .map_or(0.0, |s| f32::from(s.size) / 4.0)
    }

    fn metrics(&self, strike: usize) -> LineMetrics {
        self.strikes[strike.min(self.strikes.len() - 1)].metrics
    }

    fn strike(&self, size: f32) -> usize {
        let wanted = (size * 4.0) as i32;
        (0..self.strikes.len())
            .min_by_key(|&i| {
                let size = self.strikes[i].size;
                (i32::from(size).abs_diff(wanted), size)
            })
            .unwrap_or(0)
    }

    fn chars(&self) -> usize {
        self.chars.len()
    }

    fn index(&self, ch: char) -> Option<u32> {
        self.chars.find(ch).and_then(|i| u32::try_from(i).ok())
    }

    fn covers(&self, strike: usize, index: u32) -> bool {
        // Every strike holds every character (the format's rule).
        strike < self.strikes.len() && (index as usize) < self.chars.len()
    }

    fn advance(&self, strike: usize, index: u32) -> Option<f32> {
        let Some(glyph_table) = self.strikes.get(strike).map(|s| s.glyph_table) else {
            return Some(0.0);
        };
        if index as usize >= self.chars.len() {
            return Some(0.0);
        }
        let advance = self.read_advance(strike, glyph_table, index as usize);
        if advance.is_none() {
            self.failed.set(self.failed.get().wrapping_add(1));
        }
        advance.map(|a| f32::from(a) / 16.0)
    }

    fn render(&self, strike: usize, index: u32) -> Option<Glyph> {
        let Some(&strike) = self.strikes.get(strike) else {
            return Some(Glyph::empty(0.0));
        };
        if index as usize >= self.chars.len() {
            return Some(Glyph::empty(0.0));
        }
        let glyph = self.read_glyph(strike, index);
        if glyph.is_none() {
            self.failed.set(self.failed.get().wrapping_add(1));
        }
        glyph
    }

    fn table(&self, slot: usize) -> Option<u64> {
        self.tables
            .get(slot)
            .filter(|&&at| at != 0)
            .map(|&at| u64::from(at))
    }

    fn source(&self) -> &dyn PackSource {
        &*self.source
    }

    fn bytes(&self) -> usize {
        let advances = self.advances.try_borrow().map_or(0, |advances| {
            advances.capacity() * size_of::<Kept>()
                + advances.iter().map(Kept::bytes).sum::<usize>()
        });
        size_of::<Paged>()
            + self.strikes.capacity() * size_of::<Strike>()
            + self.chars.bytes()
            + advances
    }

    fn trim(&self) {
        let Ok(mut advances) = self.advances.try_borrow_mut() else {
            return;
        };
        for kept in advances.iter_mut() {
            *kept = match kept {
                // Still too many to keep whole: no need to try again.
                Kept::Blocks(_) => Kept::Blocks(Vec::new()),
                _ => Kept::None,
            };
        }
    }

    fn failed_reads(&self) -> u32 {
        self.failed.get()
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
            let a = stored.render(ch, size).unwrap();
            let b = in_place.glyph(ch, size);
            assert_eq!(
                (a.left, a.top, a.width, a.height),
                (b.left, b.top, b.width, b.height)
            );
            assert_eq!(a.as_ref().data, b.data);
            assert_eq!(stored.advance(ch, size), b.advance);
        }
        assert!(stored.has_char('ж') && !stored.has_char('z'));
        assert!(stored.render('z', 10.0).unwrap().width == 0);
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
        assert_eq!(stored.render('a', 10.0).unwrap().advance, 0.0);
    }

    #[test]
    fn holds_all_of_hangul_in_little_memory() {
        // 11,172 syllables of one advance, and some punctuation: more than
        // packs could hold before (8,192), read 400 records at a time.
        let metrics = LineMetrics::fallback(10.0);
        let syllables = ('\u{ac00}'..='\u{d7a3}').map(|ch| (ch, glyph(1, 255, 10.0)));
        let marks = ['\u{201c}', '\u{201d}', '\u{3001}'].map(|ch| (ch, glyph(1, 255, 4.5)));
        let mut builder = PackBuilder::new();
        builder.add_strike(10.0, metrics, marks.into_iter().chain(syllables).collect());
        let memory = Rc::new(Memory(builder.build(), Cell::new(0)));
        let card = Paged::open(memory.clone(), PackLimits::SMALL).unwrap();
        // The header, the strike, 28 blocks of records.
        assert_eq!(memory.1.get(), 2 + 11_175usize.div_ceil(BLOCK));
        let stored = StoredPack(Rc::new(card));
        assert!(stored.has_char('\u{d7a3}') && stored.has_char('\u{3001}'));
        assert!(!stored.has_char('\u{d7a4}') && !stored.has_char('a'));
        assert_eq!(stored.advance('\u{d55c}', 10.0), 10.0);
        assert_eq!(stored.advance('\u{201d}', 10.0), 4.5);
        assert_eq!(stored.render('\u{d7a3}', 10.0).unwrap().width, 1);
        // A block held whole: no bits; the advances as two runs.
        let card = Paged::open(memory, PackLimits::SMALL).unwrap();
        card.advance(0, 5);
        assert_eq!(card.chars.words.len(), 0);
        assert!(
            matches!(&card.advances.borrow()[0], Kept::Whole(Advances::Runs(r, _)) if r.len() == 2)
        );
        assert!(card.bytes() < 512, "{}", card.bytes());
    }

    /// A pack of `n` characters from U+4E00, each of its own advance.
    fn proportional(n: u32) -> Vec<u8> {
        let glyphs = (0..n)
            .filter_map(|i| {
                Some((
                    char::from_u32(0x4e00 + i)?,
                    glyph(1, 255, 1.0 + i as f32 / 16.0),
                ))
            })
            .collect();
        let mut builder = PackBuilder::new();
        builder.add_strike(10.0, LineMetrics::fallback(10.0), glyphs);
        builder.build()
    }

    #[test]
    fn reads_advances_in_blocks_past_their_budget() {
        let memory = Rc::new(Memory(proportional(5000), Cell::new(0)));
        let card = Paged::open(memory.clone(), PackLimits::SMALL).unwrap();
        let at = |i: u32| card.advance(0, i).unwrap();
        // The 4 KiB budget is passed at 2,049: none kept whole.
        assert_eq!(at(4999), 1.0 + 4999.0 / 16.0);
        assert!(matches!(&card.advances.borrow()[0], Kept::Blocks(b) if b.len() == 1));
        let reads = memory.1.get();
        // In the same block of 64: no read.
        assert_eq!(at(4992), 1.0 + 312.0);
        assert_eq!(memory.1.get(), reads);
        // 16 blocks kept (half the budget): the 17th lets the oldest go.
        let kept = card.kept_blocks();
        assert_eq!(kept, 16);
        for block in 0..kept as u32 - 1 {
            assert_eq!(at(block * 64), 1.0 + block as f32 * 4.0);
        }
        assert_eq!(memory.1.get(), reads + kept - 1);
        assert_eq!(at(4999), 1.0 + 4999.0 / 16.0);
        assert_eq!(memory.1.get(), reads + kept - 1);
        assert_eq!(at(4000), 1.0 + 250.0);
        assert_eq!(memory.1.get(), reads + kept);
        assert!(matches!(&card.advances.borrow()[0], Kept::Blocks(b) if b.len() == kept));
        // The least recently used went (block 0), the one used last stays.
        assert_eq!(at(4999), 1.0 + 4999.0 / 16.0);
        assert_eq!(memory.1.get(), reads + kept);
        assert_eq!(at(0), 1.0);
        assert_eq!(memory.1.get(), reads + kept + 1);
        assert!(card.bytes() < 4096, "{}", card.bytes());
        // Let go, still known to be read in blocks.
        card.trim();
        assert!(matches!(&card.advances.borrow()[0], Kept::Blocks(b) if b.is_empty()));
        assert_eq!(at(0), 1.0);
        assert_eq!(at(5000), 0.0);
        // 2,048 different ones fit whole.
        let card = Paged::open(
            Rc::new(Memory(proportional(2048), Cell::new(0))),
            PackLimits::SMALL,
        )
        .unwrap();
        assert_eq!(card.advance(0, 2047), Some(1.0 + 2047.0 / 16.0));
        assert!(matches!(
            &card.advances.borrow()[0],
            Kept::Whole(Advances::Each(_))
        ));
        card.trim();
        assert!(matches!(&card.advances.borrow()[0], Kept::None));
    }

    #[test]
    fn advances_that_cannot_be_kept_whole_are_read_a_block_at_a_time() {
        // No room for any whole (as where memory runs out: the same way),
        // so an advance costs at most one read of 64 records, never the
        // whole table again.
        let limits = PackLimits {
            index: 8 * 1024,
            advances: 0,
        };
        let memory = Rc::new(Memory(proportional(5000), Cell::new(0)));
        let pack = Paged::open(memory.clone(), limits).unwrap();
        let opened = memory.1.get();
        assert_eq!(pack.advance(0, 4000), Some(1.0 + 250.0));
        // One try to keep them whole (refused at once), one block.
        assert!(memory.1.get() - opened <= 2, "{}", memory.1.get() - opened);
        assert!(matches!(&pack.advances.borrow()[0], Kept::Blocks(_)));
        for i in 0..100 {
            let reads = memory.1.get();
            assert_eq!(pack.advance(0, i * 37), Some(1.0 + (i * 37) as f32 / 16.0));
            assert!(memory.1.get() - reads <= 1);
        }
        // Two blocks kept at least.
        assert_eq!(pack.kept_blocks(), 2);
    }

    /// A pack whose source fails once `fail` is set.
    struct Pulled(Vec<u8>, Cell<bool>);

    impl PackSource for Pulled {
        fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
            let at = at as usize;
            match self.0.get(at..at + buf.len()) {
                Some(bytes) if !self.1.get() => {
                    buf.copy_from_slice(bytes);
                    true
                }
                _ => false,
            }
        }
    }

    #[test]
    fn a_glyph_not_read_is_none_not_empty() {
        let card = Rc::new(Pulled(pack(), Cell::new(false)));
        let stored = StoredPack::open(card.clone()).unwrap();
        card.1.set(true);
        assert!(stored.render('ж', 10.0).is_none());
        // Not in the pack: known without reading.
        assert_eq!(stored.render('z', 10.0).map(|g| g.width), Some(0));
        card.1.set(false);
        assert_eq!(stored.render('ж', 10.0).map(|g| g.width), Some(3));
    }

    #[test]
    fn a_pack_too_scattered_to_index_is_too_large() {
        // A character every 200 code points: a segment each.
        let glyphs = (0..1000u32)
            .filter_map(|i| Some((char::from_u32(0x1_0000 + 200 * i)?, glyph(1, 255, 1.0))))
            .collect();
        let mut builder = PackBuilder::new();
        builder.add_strike(10.0, LineMetrics::fallback(10.0), glyphs);
        let memory = Rc::new(Memory(builder.build(), Cell::new(0)));
        let card = |limits| Paged::open(memory.clone(), limits).err();
        assert_eq!(card(PackLimits::SMALL), Some(FontError::TooLarge));
        assert_eq!(card(PackLimits::LARGE), None);
    }

    #[test]
    fn an_advance_not_read_is_counted_and_read_again() {
        let source = Rc::new(Pulled(proportional(5000), Cell::new(false)));
        let card = Paged::open(source.clone(), PackLimits::SMALL).unwrap();
        source.1.set(true);
        assert_eq!(card.advance(0, 10), None);
        assert_eq!(card.failed_reads(), 1);
        assert!(matches!(&card.advances.borrow()[0], Kept::None));
        source.1.set(false);
        assert_eq!(card.advance(0, 10), Some(1.0 + 10.0 / 16.0));
        // Read in blocks: a block not read is not kept either.
        source.1.set(true);
        assert_eq!(card.advance(0, 4000), None);
        source.1.set(false);
        assert_eq!(card.advance(0, 4000), Some(1.0 + 250.0));
        // Not in the pack, or no such strike: 0, nothing failed.
        assert_eq!(
            (card.advance(0, 5000), card.advance(3, 0)),
            (Some(0.0), Some(0.0))
        );
        // Through the handle: 0 where it could not be read.
        let stored = StoredPack(Rc::new(
            Paged::open(source.clone(), PackLimits::SMALL).unwrap(),
        ));
        source.1.set(true);
        assert_eq!(stored.advance('\u{4e0a}', 10.0), 0.0);
        assert!(stored.render('\u{4e0a}', 10.0).is_none());
        assert_eq!(stored.failed_reads(), 2);
        source.1.set(false);
        assert_eq!(stored.advance('\u{4e0a}', 10.0), 1.0 + 10.0 / 16.0);
        assert_eq!(stored.failed_reads(), 2);
    }

    #[test]
    fn every_strike_covers_every_character() {
        let card = Paged::open(Rc::new(Memory(pack(), Cell::new(0))), PackLimits::SMALL).unwrap();
        let index = card.index('ж').unwrap();
        assert!(card.covers(0, index) && card.covers(1, index));
        assert!(!card.covers(2, index) && !card.covers(0, 2));
        assert_eq!(card.revision(), PACK_REVISION);
    }

    #[test]
    fn refuses_what_is_no_pack() {
        let memory = Rc::new(Memory(b"nope, no pack at all".to_vec(), Cell::new(0)));
        assert!(StoredPack::open(memory).is_err());
    }
}
