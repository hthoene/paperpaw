use alloc::vec::Vec;

use crate::anchors::Anchors;
use crate::font::Font;
use crate::glyph::{Glyph, GlyphRef};

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

/// A size-bounded least-recently-used cache of glyphs rasterized at runtime.
///
/// Rasterizing is far more expensive than drawing, and a page uses only a
/// few dozen distinct glyphs, so a small cache makes page turns cheap.
/// Bitmap font packs bypass the cache: their glyphs are read in place.
#[derive(Debug)]
pub struct GlyphCache {
    entries: Vec<Entry>,
    /// Anchors of keys of packs read a glyph at a time and of outlines,
    /// at most [`ANCHORED`]: none until a mark is drawn by them.
    anchored: Vec<Anchored>,
    bytes: usize,
    capacity: usize,
    clock: u32,
}

impl GlyphCache {
    /// A cache holding at most `capacity` bytes of glyphs.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            anchored: Vec::new(),
            bytes: 0,
            capacity,
            clock: 0,
        }
    }

    /// Bytes currently used.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Lets every glyph go, and the memory that held them.
    pub fn clear(&mut self) {
        self.entries = Vec::new();
        self.anchored = Vec::new();
        self.bytes = 0;
    }

    paperpaw_table::shared! {
    /// The glyph for `ch`. `font_index` identifies `font` within the
    /// caller's font registry.
    #[cfg_attr(
        not(feature = "outline"),
        allow(
            unused_variables,
            reason = "some arguments serve only outlines, drawn with the outline feature"
        )
    )]
    pub fn glyph<'a>(
        &'a mut self,
        font_index: u16,
        font: &'a Font,
        ch: char,
        size: f32,
    ) -> GlyphRef<'a> {
        let ch = font.shown(ch);
        match font {
            Font::Bitmap(font) => font.glyph(ch, size),
            Font::Stored(font) => self.cached(font_index, ch, size, |s| font.render(ch, s)),
            #[cfg(feature = "outline")]
            Font::Outline(font) => self.cached(font_index, ch, size, |s| font.rasterize(ch, s)),
        }
    } @ text::CACHE_GLYPH
    }

    /// Where marks sit on `ch` (see [`crate::anchors`]): read in place
    /// from packs in memory, else once and kept.
    pub(crate) fn anchors(&mut self, font_index: u16, font: &Font, ch: char, size: f32) -> Anchors {
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

    fn cached(
        &mut self,
        font_index: u16,
        ch: char,
        size: f32,
        rasterize: impl FnOnce(f32) -> Glyph,
    ) -> GlyphRef<'_> {
        let key = Key {
            font: font_index,
            size: (size * 4.0) as u16,
            ch,
        };
        self.clock = self.clock.wrapping_add(1);
        let index = match self.entries.iter().position(|e| e.key == key) {
            Some(index) => index,
            None => {
                let glyph = rasterize(f32::from(key.size) / 4.0);
                self.make_room(glyph.byte_size());
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
        entry.glyph.as_ref()
    }

    fn make_room(&mut self, needed: usize) {
        while self.bytes + needed > self.capacity && !self.entries.is_empty() {
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
        let mut cache = GlyphCache::new(3 * core::mem::size_of::<Glyph>());
        let make = |_| Glyph::empty(1.0);
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
        let mut cache = GlyphCache::new(4096);
        for _ in 0..2 {
            for &ch in &chars {
                assert_eq!(cache.anchors(0, &font, ch, 10.0), of(ch), "{ch}");
            }
        }
        assert_eq!(cache.anchored.len(), ANCHORED);
        assert_eq!(cache.anchored.capacity(), ANCHORED);
        // At most this much, only once marks were drawn by anchors.
        assert!(ANCHORED * core::mem::size_of::<Anchored>() <= 2600);
        assert_eq!(GlyphCache::new(10).anchored.capacity(), 0);
    }
}
