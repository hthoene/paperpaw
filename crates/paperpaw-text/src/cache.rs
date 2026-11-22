use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::anchors::Anchors;
use crate::font::Font;
use crate::glyph::{Glyph, GlyphRef};

/// What a glyph that could not be read now is drawn as (with its
/// advance, where it is known).
const NOT_READ: GlyphRef<'static> = GlyphRef {
    format: crate::glyph::COVERAGE4,
    left: 0,
    top: 0,
    width: 0,
    height: 0,
    advance: 0.0,
    data: &[],
};

/// Bitmaps of glyphs let go that a cache keeps at most, for the next glyph
/// read from a pack on a card to be read into: a full cache then takes no
/// memory for it, and leaves no holes in the heap.
const SPARE: usize = 2;

/// The largest bitmap kept for that, in bytes (a glyph of 64 px).
const SPARE_MAX: usize = 2048;

/// Room made for this many entries at a time when the list is full: no
/// doubling past what the cache holds.
const ENTRY_STEP: usize = 32;

/// The keys whose anchors the cache keeps at most: the letters and marks
/// of a few pointed words (anchors are looked up for marks alone).
const ANCHORED: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    font: u16,
    /// Size in quarter pixels.
    size: u16,
    ch: char,
}

#[derive(Debug)]
struct Entry {
    key: Key,
    glyph: Glyph,
    last_used: u32,
}

/// The anchors of a key, read once.
#[derive(Debug)]
struct Anchored {
    key: Key,
    anchors: Anchors,
    last_used: u32,
}

/// What a glyph cache does, behind a trait object so that how it keeps
/// glyphs (its eviction, how it reads packs on cards) can change in any
/// later firmware without changing the plugin interface, which pins these
/// items, not the type behind them.
///
/// Items are only ever appended, each with a higher [`Cache::revision`]:
/// code calls an item that came with revision `n` only where the cache's
/// revision is `n` or more. No supertraits, generic or `Self`-returning
/// items: its table's order is its items' (ADR 0026).
trait Cache {
    /// The items it has: those of this revision and before
    /// ([`CACHE_REVISION`] here).
    fn revision(&self) -> u16;
    fn glyph<'a>(
        &'a mut self,
        font_index: u16,
        font: &'a Font,
        ch: char,
        size: f32,
    ) -> GlyphRef<'a>;
    fn anchors(&mut self, font_index: u16, font: &Font, ch: char, size: f32) -> Anchors;
    fn bytes(&self) -> usize;
    fn capacity(&self) -> usize;
    fn set_capacity(&mut self, capacity: usize);
    fn trim(&mut self, keep: usize);
    fn clear(&mut self);
}

/// The revision of [`Cache`] that this code has.
const CACHE_REVISION: u16 = 1;

/// A size-bounded cache of glyphs rasterized at runtime or read from packs
/// on a card: a handle, made by the firmware.
///
/// Rasterizing is far more expensive than drawing, and a page uses only a
/// few dozen distinct glyphs, so a small cache makes page turns cheap.
/// Bitmap font packs bypass the cache: their glyphs are read in place.
pub struct GlyphCache(Box<dyn Cache>);

impl fmt::Debug for GlyphCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphCache")
            .field("revision", &self.0.revision())
            .field("bytes", &self.0.bytes())
            .field("capacity", &self.0.capacity())
            .finish()
    }
}

impl GlyphCache {
    /// A cache holding at most `capacity` bytes of glyphs: the firmware's
    /// code, so that how it keeps them is the firmware's too.
    pub fn new(capacity: usize) -> GlyphCache {
        GlyphCache(Box::new(Lru::new(capacity)))
    }

    /// Bytes currently used.
    pub fn bytes(&self) -> usize {
        self.0.bytes()
    }

    /// The most bytes it holds.
    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }

    /// Holds at most `capacity` bytes from now on (e.g. more while a
    /// script's pack is read from a card, whose glyphs are dear to read
    /// again), letting the least recently used go at once.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.0.set_capacity(capacity);
    }

    /// Lets the least recently used glyphs go until it holds at most
    /// `keep` bytes, for when memory is short; its capacity stays.
    pub fn trim(&mut self, keep: usize) {
        self.0.trim(keep);
    }

    /// Lets every glyph go, and the memory that held them.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The glyph for `ch`. `font_index` identifies `font` within the
    /// caller's font registry.
    pub fn glyph<'a>(
        &'a mut self,
        font_index: u16,
        font: &'a Font,
        ch: char,
        size: f32,
    ) -> GlyphRef<'a> {
        self.0.glyph(font_index, font, ch, size)
    }

    /// Where marks sit on `ch` (see [`crate::anchors`]): read in place
    /// from packs in memory, else once and kept.
    pub(crate) fn anchors(&mut self, font_index: u16, font: &Font, ch: char, size: f32) -> Anchors {
        self.0.anchors(font_index, font, ch, size)
    }
}

/// The most a bitmap of a cold glyph is, in bytes, for the glyph cache to keep it.
/// A clock's digits are larger and are not kept (see `Lru::large`).
#[cfg(feature = "kernel")]
const KEPT: usize = 1024;

/// The bitmap of the cold glyph `found` of a built-in pack whose bitmaps are
/// deflated (`deflated.rs`), inflated out of the block or the two it lies in
/// into `bitmap`; `None` where that could not be done. The block is held while
/// it is read and let go, so that nothing but the glyph stays in memory.
#[cfg(feature = "kernel")]
fn read_deflated(
    font: &crate::bitmap::BitmapFont,
    found: crate::bitmap::Located,
    mut bitmap: Vec<u8>,
) -> Option<Glyph> {
    use crate::deflated::BLOCK;
    let (deflated, pack) = font.deflated()?;
    bitmap.clear();
    bitmap.try_reserve_exact(found.len).ok()?;
    let end = found.start.checked_add(found.len)?;
    let (mut decoder, mut block) = (Vec::new(), Vec::new());
    let mut at = found.start;
    while at < end {
        let number = at / BLOCK;
        if !deflated.inflate(pack, number, &mut decoder, &mut block) {
            return None;
        }
        let from = at - number * BLOCK;
        let to = (end - number * BLOCK).min(block.len());
        let piece = block.get(from..to).filter(|piece| !piece.is_empty())?;
        bitmap.extend_from_slice(piece);
        at = number * BLOCK + to;
    }
    Some(Glyph {
        format: found.format,
        left: found.left,
        top: found.top,
        width: found.width,
        height: found.height,
        advance: found.advance,
        data: bitmap,
    })
}

/// The least recently used glyphs go first.
#[derive(Debug)]
struct Lru {
    entries: Vec<Entry>,
    /// Anchors of keys of packs read a glyph at a time and of outlines,
    /// at most [`ANCHORED`]: none until a mark is drawn by them.
    anchored: Vec<Anchored>,
    bytes: usize,
    capacity: usize,
    clock: u32,
    /// Bitmaps of glyphs let go, at most [`SPARE`] (see there).
    spare: Vec<Vec<u8>>,
    /// The glyph of such a pack too large to keep (a clock's digit), until the
    /// next glyph is asked for.
    #[cfg(feature = "kernel")]
    large: Option<Glyph>,
}

impl Cache for Lru {
    fn revision(&self) -> u16 {
        CACHE_REVISION
    }

    #[cfg_attr(
        not(feature = "outline"),
        allow(
            unused_variables,
            reason = "some arguments serve only outlines, drawn with the outline feature"
        )
    )]
    fn glyph<'a>(
        &'a mut self,
        font_index: u16,
        font: &'a Font,
        ch: char,
        size: f32,
    ) -> GlyphRef<'a> {
        let ch = font.shown(ch);
        match font {
            #[cfg(feature = "kernel")]
            Font::Bitmap(font) if font.is_deflated() => {
                self.deflated_glyph(font_index, font, ch, size)
            }
            Font::Bitmap(font) => font.glyph(ch, size),
            // Not read now (the card pulled): drawn as nothing, its
            // advance kept, so that the line stays as it was measured.
            Font::Stored(font) => match self.cached_with(font_index, ch, size, |s, spare| {
                font.render_reusing(ch, s, spare)
            }) {
                Some(glyph) => glyph,
                None => GlyphRef {
                    advance: font.advance(ch, size),
                    ..NOT_READ
                },
            },
            #[cfg(feature = "outline")]
            Font::Outline(font) => self
                .cached(font_index, ch, size, |s| Some(font.rasterize(ch, s)))
                .unwrap_or(NOT_READ),
        }
    }

    fn anchors(&mut self, font_index: u16, font: &Font, ch: char, size: f32) -> Anchors {
        if let Font::Bitmap(_) = font {
            return font.anchors(ch, size);
        }
        let key = Key {
            font: font_index,
            size: (size * 4.0) as u16,
            ch,
        };
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        if let Some(entry) = self.anchored.iter_mut().find(|e| e.key == key) {
            entry.last_used = clock;
            return entry.anchors;
        }
        let anchors = font.anchors(ch, f32::from(key.size) / 4.0);
        let entry = Anchored {
            key,
            anchors,
            last_used: clock,
        };
        if self.anchored.len() < ANCHORED {
            // At once the room for all: no doubling past it. Where the heap
            // refuses, the anchors are answered and not kept.
            if self
                .anchored
                .try_reserve_exact(ANCHORED - self.anchored.len())
                .is_ok()
            {
                self.anchored.push(entry);
            }
        } else if let Some(oldest) = self
            .anchored
            .iter_mut()
            .max_by_key(|e| clock.wrapping_sub(e.last_used))
        {
            *oldest = entry;
        }
        anchors
    }

    fn bytes(&self) -> usize {
        self.bytes
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.shrink_to(capacity);
    }

    fn trim(&mut self, keep: usize) {
        self.shrink_to(keep);
        self.spare = Vec::new();
        #[cfg(feature = "kernel")]
        self.let_blocks_go();
        self.entries.shrink_to_fit();
    }

    fn clear(&mut self) {
        #[cfg(feature = "kernel")]
        self.let_blocks_go();
        self.entries = Vec::new();
        self.anchored = Vec::new();
        self.spare = Vec::new();
        self.bytes = 0;
    }
}

impl Lru {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            anchored: Vec::new(),
            bytes: 0,
            capacity,
            clock: 0,
            spare: Vec::new(),
            #[cfg(feature = "kernel")]
            large: None,
        }
    }

    /// The glyph of a built-in pack with deflated bitmaps (`deflated.rs`): a
    /// hot one is read in place as ever; a cold one (an icon, a letter few
    /// texts use) is inflated once and kept like a glyph read from a card (text
    /// draws a glyph again and again), unless it is large (a clock's digit),
    /// which is held until the next glyph is asked for. Not read now (no memory,
    /// a damaged block): drawn as nothing, its advance kept.
    #[cfg(feature = "kernel")]
    fn deflated_glyph<'a>(
        &'a mut self,
        font_index: u16,
        font: &'a crate::bitmap::BitmapFont,
        ch: char,
        size: f32,
    ) -> GlyphRef<'a> {
        self.large = None;
        let Some(found) = font.locate(ch, size) else {
            return font.glyph(ch, size);
        };
        if !found.cold {
            return font.glyph_of(found);
        }
        if found.len > KEPT {
            self.large = read_deflated(font, found, Vec::new());
            return self.large.as_ref().map_or(
                GlyphRef {
                    advance: found.advance,
                    ..NOT_READ
                },
                Glyph::as_ref,
            );
        }
        match self.cached_with(font_index, ch, size, |s, spare| {
            let found = font.locate(ch, s)?;
            read_deflated(font, found, spare)
        }) {
            Some(glyph) => glyph,
            None => GlyphRef {
                advance: found.advance,
                ..NOT_READ
            },
        }
    }

    /// Lets the held glyph go, and the memory that held it.
    #[cfg(feature = "kernel")]
    fn let_blocks_go(&mut self) {
        self.large = None;
    }

    /// The glyph of a key, made by `rasterize` if it is not kept: `None`
    /// where it could not be made now, which is not kept.
    #[cfg(any(test, feature = "outline"))]
    fn cached(
        &mut self,
        font_index: u16,
        ch: char,
        size: f32,
        rasterize: impl FnOnce(f32) -> Option<Glyph>,
    ) -> Option<GlyphRef<'_>> {
        self.cached_with(font_index, ch, size, |size, _| rasterize(size))
    }

    /// [`Lru::cached`], where `rasterize` is also given the bitmap of a
    /// glyph let go before (empty at first) to make the new one in.
    fn cached_with(
        &mut self,
        font_index: u16,
        ch: char,
        size: f32,
        rasterize: impl FnOnce(f32, Vec<u8>) -> Option<Glyph>,
    ) -> Option<GlyphRef<'_>> {
        let key = Key {
            font: font_index,
            size: (size * 4.0) as u16,
            ch,
        };
        self.clock = self.clock.wrapping_add(1);
        let index = match self.entries.iter().position(|e| e.key == key) {
            Some(index) => index,
            None => {
                let spare = self.spare.pop().unwrap_or_default();
                let glyph = rasterize(f32::from(key.size) / 4.0, spare)?;
                self.shrink_to(self.capacity.saturating_sub(glyph.byte_size()));
                // Room for the entry before it is counted: where the heap
                // refuses, the glyph is not kept and reads as not read now.
                if self.entries.len() == self.entries.capacity() {
                    self.entries.try_reserve_exact(ENTRY_STEP).ok()?;
                }
                self.bytes += glyph.byte_size();
                self.entries.push(Entry {
                    key,
                    glyph,
                    last_used: 0,
                });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[index];
        entry.last_used = self.clock;
        Some(entry.glyph.as_ref())
    }

    /// Lets the least recently used go until at most `keep` bytes are
    /// held.
    fn shrink_to(&mut self, keep: usize) {
        while self.bytes > keep && !self.entries.is_empty() {
            let clock = self.clock;
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .max_by_key(|(_, e)| clock.wrapping_sub(e.last_used))
                .map_or(0, |(i, _)| i);
            let mut removed = self.entries.swap_remove(oldest);
            self.bytes -= removed.glyph.byte_size();
            let data = core::mem::take(&mut removed.glyph.data);
            if self.spare.len() < SPARE && (1..=SPARE_MAX).contains(&data.capacity()) {
                if self.spare.capacity() == 0 && self.spare.try_reserve_exact(SPARE).is_err() {
                    continue;
                }
                self.spare.push(data);
            }
        }
    }
}

#[cfg(test)]
#[path = "cache_oom_tests.rs"]
mod oom_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut cache = Lru::new(3 * core::mem::size_of::<Glyph>());
        let make = |_| Some(Glyph::empty(1.0));
        cache.cached(0, 'a', 10.0, make);
        cache.cached(0, 'b', 10.0, make);
        cache.cached(0, 'c', 10.0, make);
        cache.cached(0, 'a', 10.0, make); // touch 'a'
        cache.cached(0, 'd', 10.0, make); // evicts 'b'
        let keys: Vec<char> = cache.entries.iter().map(|e| e.key.ch).collect();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&'a') && keys.contains(&'d') && !keys.contains(&'b'));
    }

    #[test]
    fn a_smaller_capacity_lets_the_oldest_go_at_once() {
        let one = core::mem::size_of::<Glyph>();
        let mut cache = GlyphCache::new(3 * one);
        let mut lru = Lru::new(3 * one);
        let make = |_| Some(Glyph::empty(1.0));
        for ch in ['a', 'b', 'c'] {
            lru.cached(0, ch, 10.0, make);
        }
        lru.cached(0, 'a', 10.0, make); // touch 'a'
        lru.set_capacity(one);
        let keys: Vec<char> = lru.entries.iter().map(|e| e.key.ch).collect();
        assert_eq!(keys, ['a']);
        assert_eq!((lru.capacity(), lru.bytes()), (one, one));
        cache.set_capacity(5 * one);
        assert_eq!((cache.capacity(), cache.bytes()), (5 * one, 0));
        // Trimmed: the capacity stays.
        for ch in ['a', 'b', 'c'] {
            lru.cached(0, ch, 10.0, make);
        }
        lru.trim(0);
        assert_eq!(
            (lru.capacity(), lru.bytes(), lru.entries.len()),
            (one, 0, 0)
        );
    }

    #[test]
    fn the_memory_of_a_glyph_let_go_is_given_to_the_next() {
        let make = |_: f32, spare: Vec<u8>| {
            let mut glyph = Glyph::empty(1.0);
            glyph.data = spare;
            glyph.data.clear();
            glyph.data.resize(100, 7);
            Some(glyph)
        };
        let room = 2 * (100 + core::mem::size_of::<Glyph>());
        let mut cache = Lru::new(room);
        let mut seen = Vec::new();
        for ch in ['a', 'b', 'c', 'd', 'e', 'f'] {
            let before = cache.spare.len();
            let got = cache
                .cached_with(0, ch, 10.0, |s, spare| {
                    seen.push((spare.capacity(), 0));
                    make(s, spare)
                })
                .map(|g| g.advance);
            assert_eq!(got, Some(1.0));
            // Kept bytes stay within the room, the spare is one buffer.
            assert!(cache.bytes() <= room, "{ch}");
            assert!(cache.spare.len() <= SPARE && before <= SPARE);
        }
        // From the third glyph on, the memory of one let go is what a new one
        // is read into: it comes with room.
        assert_eq!(seen[0].0, 0);
        assert!(seen[3..].iter().all(|&(capacity, _)| capacity >= 100));
        cache.trim(0);
        assert!(cache.spare.is_empty());
    }

    #[test]
    fn a_glyph_not_read_is_not_kept() {
        let mut cache = Lru::new(4096);
        assert!(cache.cached(0, 'a', 10.0, |_| None).is_none());
        assert!(cache.entries.is_empty());
        let glyph = cache.cached(0, 'a', 10.0, |_| Some(Glyph::empty(2.0)));
        assert_eq!(glyph.map(|g| g.advance), Some(2.0));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.revision(), CACHE_REVISION);
    }

    #[test]
    fn keeps_the_anchors_of_a_few_keys_in_little_memory() {
        use crate::anchors::{self, Anchor, Anchors};
        use crate::{BitmapFont, LineMetrics, PackBuilder, PackSource, StoredPack};

        struct Memory(Vec<u8>);
        impl PackSource for Memory {
            fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
                let at = at as usize;
                match self.0.get(at..at + buf.len()) {
                    Some(b) => {
                        buf.copy_from_slice(b);
                        true
                    }
                    None => false,
                }
            }
        }
        let chars: Vec<char> = ('\u{5d0}'..='\u{5d0}')
            .chain('\u{600}'..'\u{640}')
            .collect();
        let of = |ch: char| {
            let mut a = Anchors::NONE;
            a.push(Anchor {
                class: 0,
                x: (u32::from(ch) % 100) as i8,
                y: 1,
            });
            a
        };
        let mut builder = PackBuilder::new();
        let metrics = LineMetrics::fallback(10.0);
        builder.add_strike(
            10.0,
            metrics,
            chars.iter().map(|&c| (c, Glyph::empty(1.0))).collect(),
        );
        let keys = alloc::vec![chars.iter().map(|&c| (c, of(c))).collect()];
        builder.table(BitmapFont::ANCHORS, 1, &anchors::build(&keys));
        let pack = StoredPack::open(alloc::rc::Rc::new(Memory(builder.build()))).unwrap();
        let font = Font::Stored(pack);
        let mut cache = Lru::new(4096);
        for _ in 0..2 {
            for &ch in &chars {
                assert_eq!(cache.anchors(0, &font, ch, 10.0), of(ch), "{ch}");
            }
        }
        assert_eq!(cache.anchored.len(), ANCHORED);
        assert_eq!(cache.anchored.capacity(), ANCHORED);
        // At most this much, only once marks were drawn by anchors.
        assert!(ANCHORED * core::mem::size_of::<Anchored>() <= 2600);
        assert_eq!(Lru::new(10).anchored.capacity(), 0);
    }
}
