use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::anchors::Anchors;
use crate::font::Font;
use crate::glyph::{Glyph, GlyphRef};

/// What a glyph that could not be read now is drawn as (with its
/// advance, where it is known).
const NOT_READ: GlyphRef<'static> = GlyphRef {
    left: 0,
    top: 0,
    width: 0,
    height: 0,
    advance: 0.0,
    data: &[],
};

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
    paperpaw_table::shared! {
    /// A cache holding at most `capacity` bytes of glyphs: the firmware's
    /// code, so that how it keeps them is the firmware's too.
    pub fn new(capacity: usize) -> GlyphCache {
        GlyphCache(Box::new(Lru::new(capacity)))
    } @ text::CACHE_NEW
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
            Font::Bitmap(font) => font.glyph(ch, size),
            // Not read now (the card pulled): drawn as nothing, its
            // advance kept, so that the line stays as it was measured.
            Font::Stored(font) => match self.cached(font_index, ch, size, |s| font.render(ch, s)) {
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
            // At once the room for all: no doubling past it.
            self.anchored.reserve_exact(ANCHORED - self.anchored.len());
            self.anchored.push(entry);
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
        self.entries.shrink_to_fit();
    }

    fn clear(&mut self) {
        self.entries = Vec::new();
        self.anchored = Vec::new();
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
        }
    }

    /// The glyph of a key, made by `rasterize` if it is not kept: `None`
    /// where it could not be made now, which is not kept.
    fn cached(
        &mut self,
        font_index: u16,
        ch: char,
        size: f32,
        rasterize: impl FnOnce(f32) -> Option<Glyph>,
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
                let glyph = rasterize(f32::from(key.size) / 4.0)?;
                self.shrink_to(self.capacity.saturating_sub(glyph.byte_size()));
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
            let removed = self.entries.swap_remove(oldest);
            self.bytes -= removed.glyph.byte_size();
        }
    }
}

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
