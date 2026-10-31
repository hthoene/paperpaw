//! Bitmap font packs (`.ppfp`) from cards and plugins: read in place
//! ([`BitmapFont`]) or a glyph at a time ([`StoredPack`]). The version 1
//! test pack is read by both; damaged packs (random bytes, wrong counts,
//! offsets past the end, unsorted or invalid characters) are refused or
//! read without panicking; and the glyph cache stays within its budget,
//! evicts the least recently used glyph and never hands out a wrong one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_text::{BitmapFont, Font, FontError, Glyph, GlyphCache, PackSource, StoredPack};
use proptest::prelude::*;

/// The version 1 test pack: strikes of 12 and 24.5 px with `?`, `A`, `é`.
const PACK: &[u8] = include_bytes!("../testdata/pack.ppfp.v1");
/// Where the header keeps the strike count, and each strike its fields.
const STRIKES: usize = 8;
const STRIKE: usize = 12;
const GLYPHS: usize = STRIKE + 16;
const TABLE: usize = STRIKE + 20;
const BITMAPS: usize = STRIKE + 24;

/// A pack in memory that counts reads and remembers the largest.
struct Memory {
    data: Vec<u8>,
    reads: Cell<usize>,
    largest: Cell<usize>,
}

impl Memory {
    fn new(data: &[u8]) -> Rc<Memory> {
        Rc::new(Memory {
            data: data.to_vec(),
            reads: Cell::new(0),
            largest: Cell::new(0),
        })
    }
}

impl PackSource for Memory {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        self.reads.set(self.reads.get() + 1);
        self.largest.set(self.largest.get().max(buf.len()));
        let Ok(at) = usize::try_from(at) else {
            return false;
        };
        match at
            .checked_add(buf.len())
            .and_then(|end| self.data.get(at..end))
        {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

fn put_u16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn u32_at(data: &[u8], at: usize) -> usize {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize
}

fn open(data: &[u8]) -> Result<StoredPack, FontError> {
    StoredPack::open(Memory::new(data))
}

const CHARS: [char; 8] = ['?', 'A', 'é', '\u{fffd}', 'Ж', '\0', ' ', char::MAX];
const SIZES: [f32; 7] = [0.0, 12.0, 24.5, 18.0, 1.0e9, -3.0, f32::NAN];

/// Uses every way of reading a pack that was accepted; none may panic.
fn read_all(font: &BitmapFont) {
    let _ = font.sizes().count();
    for size in SIZES {
        let _ = font.line_metrics(size);
        for ch in CHARS {
            let glyph = font.glyph(ch, size);
            let _ = (font.advance(ch, size), font.has_char(ch));
            // Corners and outside: truncated data reads as empty.
            for (x, y) in [(0, 0), (glyph.width, glyph.height), (u16::MAX, u16::MAX)] {
                let _ = glyph.coverage(x.saturating_sub(1), y.saturating_sub(1));
            }
        }
    }
}

fn read_all_stored(pack: &StoredPack) {
    for size in SIZES {
        let _ = pack.line_metrics(size);
        for ch in CHARS {
            let glyph = pack.render(ch, size).unwrap_or_else(|| Glyph::empty(0.0));
            let _ = (pack.advance(ch, size), pack.has_char(ch));
            let glyph = glyph.as_ref();
            let _ = glyph.coverage(
                glyph.width.saturating_sub(1),
                glyph.height.saturating_sub(1),
            );
        }
    }
}

#[test]
fn the_version_1_pack_reads_the_same_in_place_and_stored() {
    let font = BitmapFont::new(PACK).unwrap();
    let stored = open(PACK).unwrap();
    for (ch, size) in [('A', 12.0), ('?', 13.0), ('é', 24.5), ('A', 30.0)] {
        let a = font.glyph(ch, size);
        let b = stored.render(ch, size).unwrap();
        assert_eq!(a, b.as_ref(), "{ch} at {size}");
        assert_eq!(stored.advance(ch, size), a.advance);
    }
    assert_eq!(stored.line_metrics(24.0).ascent, 19.0);
    assert!(stored.has_char('é') && !stored.has_char('B'));
    // Characters it lacks: in place '?' stands in, stored ones are empty.
    assert_eq!(font.glyph('B', 12.0).width, 2);
    assert!(stored.render('B', 12.0).unwrap().as_ref().is_empty());
    assert_eq!(stored.advance('B', 12.0), 0.0);
}

#[test]
fn packs_under_the_old_name_and_other_versions() {
    let mut old = PACK.to_vec();
    old[..4].copy_from_slice(b"QIFP");
    assert!(BitmapFont::new(old.clone()).is_ok());
    assert!(open(&old).is_ok());
    let mut newer = PACK.to_vec();
    newer[4] = 3;
    assert_eq!(
        BitmapFont::new(newer.clone()).unwrap_err(),
        FontError::UnsupportedVersion
    );
    assert_eq!(open(&newer).unwrap_err(), FontError::UnsupportedVersion);
    assert!(BitmapFont::is_pack(PACK) && !BitmapFont::is_pack(b"PPF"));
    // A font file is no pack: loading it as one says so.
    assert!(matches!(
        Font::from_data(&b"PPFP"[..], &[]),
        Err(FontError::Invalid)
    ));
}

#[test]
fn packs_without_strikes_or_header_are_refused() {
    for len in 0..12 {
        assert!(
            BitmapFont::new(PACK[..len].to_vec()).is_err(),
            "{len} bytes"
        );
        assert!(open(&PACK[..len]).is_err(), "{len} bytes");
    }
    let mut none = PACK.to_vec();
    put_u16(&mut none, STRIKES, 0);
    assert_eq!(
        BitmapFont::new(none.clone()).unwrap_err(),
        FontError::Invalid
    );
    assert_eq!(open(&none).unwrap_err(), FontError::Invalid);
}

#[test]
fn counts_and_offsets_past_the_end_are_refused() {
    let cases: [(&str, usize, u32); 5] = [
        ("strikes", STRIKES, 0xffff),
        ("glyph count", GLYPHS, u32::MAX),
        ("glyph count", GLYPHS, 9000),
        ("glyph table", TABLE, u32::MAX - 8),
        ("glyph table", TABLE, PACK.len() as u32 - 10),
    ];
    for (what, at, value) in cases {
        let mut data = PACK.to_vec();
        if at == STRIKES {
            put_u16(&mut data, at, value as u16);
        } else {
            put_u32(&mut data, at, value);
        }
        assert!(BitmapFont::new(data.clone()).is_err(), "{what} {value}");
        assert!(open(&data).is_err(), "{what} {value}");
    }
    // Bitmaps past the end: refused in place; stored, glyphs read empty
    // but keep their advance.
    let mut data = PACK.to_vec();
    put_u32(&mut data, BITMAPS, u32::MAX);
    assert!(BitmapFont::new(data.clone()).is_err());
    let stored = open(&data).unwrap();
    assert!(stored.render('A', 12.0).is_none());
    assert_eq!(stored.advance('A', 12.0), 7.5);
    // Drawn as nothing, its advance kept, and not kept as its glyph.
    let font = Font::Stored(stored);
    let mut cache = GlyphCache::new(4096);
    let glyph = cache.glyph(0, &font, 'A', 12.0);
    assert!(glyph.is_empty());
    assert_eq!(glyph.advance, 7.5);
    assert_eq!(cache.bytes(), 0);
}

#[test]
fn glyph_data_past_the_end_draws_nothing() {
    let mut data = PACK.to_vec();
    // The data offset of the first strike's 'A' (second record).
    let record = u32_at(&data, TABLE) + 20;
    put_u32(&mut data, record + 16, 0xffff_0000);
    let font = BitmapFont::new(data.clone()).unwrap();
    assert!(font.glyph('A', 12.0).is_empty());
    assert_eq!(font.glyph('?', 12.0).width, 2, "the others are still read");
    let stored = open(&data).unwrap();
    assert!(stored.render('A', 12.0).is_none());
    // A glyph claiming more rows than its data has.
    let mut data = PACK.to_vec();
    put_u16(&mut data, record + 14, 500);
    let font = BitmapFont::new(data.clone()).unwrap();
    assert!(font.glyph('A', 12.0).is_empty());
    read_all(&font);
}

#[test]
fn stored_packs_refuse_unsorted_invalid_or_uneven_characters() {
    let table = u32_at(PACK, TABLE);
    // 'A' before '?'.
    let mut unsorted = PACK.to_vec();
    put_u32(&mut unsorted, table, 'Z' as u32);
    assert!(open(&unsorted).is_err());
    // In place the pack is read; lookups just miss.
    read_all(&BitmapFont::new(unsorted).unwrap());
    // A surrogate is no character.
    let mut invalid = PACK.to_vec();
    put_u32(&mut invalid, table, 0xd800);
    assert!(open(&invalid).is_err());
    read_all(&BitmapFont::new(invalid).unwrap());
    // Strikes with different numbers of characters.
    let mut uneven = PACK.to_vec();
    put_u32(&mut uneven, STRIKE + 32 + 16, 2);
    assert!(open(&uneven).is_err());
}

/// A damaged header claiming 65535 strikes in a pack of 211 bytes must
/// not make `StoredPack::open` allocate a 2 MiB strike table before it
/// reads anything: on the X4 (about 380 KB of RAM) a broken pack on the
/// card would stop the device instead of being refused.
#[test]
fn opening_a_stored_pack_allocates_no_more_than_it_needs() {
    let mut data = PACK.to_vec();
    put_u16(&mut data, STRIKES, 0xffff);
    let memory = Memory::new(&data);
    assert!(StoredPack::open(memory.clone()).is_err());
    assert!(
        memory.largest.get() <= 256 * 1024,
        "asked for {} bytes at once",
        memory.largest.get()
    );
}

/// A damaged glyph record claiming a 4096 by 4096 glyph must not make
/// `render` allocate 8 MiB before reading it (65535 by 65535: 2 GiB).
#[test]
fn rendering_a_damaged_stored_glyph_allocates_no_more_than_it_needs() {
    let mut data = PACK.to_vec();
    let record = u32_at(&data, TABLE) + 20;
    put_u16(&mut data, record + 12, 4096);
    put_u16(&mut data, record + 14, 4096);
    let memory = Memory::new(&data);
    let stored = StoredPack::open(memory.clone()).unwrap();
    let glyph = stored.render('A', 12.0).unwrap();
    assert!(glyph.as_ref().is_empty());
    assert!(
        memory.largest.get() <= 256 * 1024,
        "asked for {} bytes at once",
        memory.largest.get()
    );
}

/// The strike nearest a size is found by its distance in quarter pixels:
/// sizes below about -5e8 px (e.g. a broken setting) must not overflow
/// that reckoning, which would panic in debug builds.
#[test]
fn sizes_far_below_zero_do_not_overflow() {
    let font = BitmapFont::new(PACK).unwrap();
    let stored = open(PACK).unwrap();
    for size in [f32::NEG_INFINITY, -1.0e10] {
        assert_eq!(font.glyph('A', size).width, 3, "the smallest strike");
        assert_eq!(stored.advance('A', size), 7.5);
    }
}

fn mutated() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec((0..PACK.len(), any::<u8>(), 0u8..4), 1..6).prop_map(|changes| {
        let mut data = PACK.to_vec();
        for (at, value, how) in changes {
            if how == 0 {
                data.truncate(at.max(12));
                continue;
            }
            let at = at.min(data.len() - 1);
            if how == 1 {
                data[at] ^= 0x80;
            } else {
                data[at] = value;
            }
        }
        data
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_bytes_are_refused_or_read_safely(
        tail in proptest::collection::vec(any::<u8>(), 0..300),
        magic in any::<bool>(),
    ) {
        let mut data = if magic { b"PPFP\x01\0\0\0".to_vec() } else { Vec::new() };
        data.extend_from_slice(&tail);
        if let Ok(font) = BitmapFont::new(data.clone()) {
            read_all(&font);
        }
        if let Ok(pack) = open(&data) {
            read_all_stored(&pack);
        }
    }

    #[test]
    fn damaged_packs_are_refused_or_read_safely(data in mutated()) {
        if let Ok(font) = BitmapFont::new(data.clone()) {
            read_all(&font);
        }
        if let Ok(pack) = open(&data) {
            read_all_stored(&pack);
        }
    }

    /// Whatever the budget and order, the cache hands out the glyph the
    /// font renders, and keeps within its budget unless a single glyph is
    /// larger than all of it.
    #[test]
    fn the_glyph_cache_keeps_its_budget_and_hands_out_the_right_glyph(
        capacity in 0usize..600,
        uses in proptest::collection::vec((0u16..3, 0usize..4, 0usize..4), 1..60),
    ) {
        let pack = open(PACK).unwrap();
        let font = Font::Stored(pack.clone());
        let mut cache = GlyphCache::new(capacity);
        let chars = ['?', 'A', 'é', 'x'];
        let sizes = [12.0, 24.5, 12.1, 30.0];
        let largest = chars
            .iter()
            .flat_map(|&c| sizes.iter().map(move |&s| (c, s)))
            .map(|(c, s)| pack.render(c, s).unwrap().byte_size())
            .max()
            .unwrap();
        for (index, c, s) in uses {
            let (ch, size) = (chars[c], sizes[s]);
            // The cache keeps sizes in quarter pixels.
            let expected = pack.render(ch, (size * 4.0).trunc() / 4.0).unwrap();
            let got = cache.glyph(index, &font, ch, size);
            prop_assert_eq!(got, expected.as_ref());
            prop_assert!(cache.bytes() <= capacity.max(largest), "{} bytes", cache.bytes());
        }
        cache.clear();
        prop_assert_eq!(cache.bytes(), 0);
    }
}

#[test]
fn the_glyph_cache_evicts_the_least_recently_used() {
    let memory = Memory::new(PACK);
    let pack = StoredPack::open(memory.clone()).unwrap();
    let size = |ch| pack.render(ch, 12.0).unwrap().byte_size();
    let font = Font::Stored(pack.clone());
    // Room for exactly the three glyphs of the small strike.
    let mut cache = GlyphCache::new(size('?') + size('A') + size('é'));
    let reads = |cache: &mut GlyphCache, ch: char| {
        let before = memory.reads.get();
        cache.glyph(0, &font, ch, 12.0);
        memory.reads.get() - before
    };
    for ch in ['?', 'A', 'é'] {
        assert!(reads(&mut cache, ch) > 0, "{ch} is read the first time");
    }
    assert_eq!(cache.bytes(), size('?') + size('A') + size('é'));
    // '?' used again: 'A' is now the oldest.
    assert_eq!(reads(&mut cache, '?'), 0, "'?' comes from the cache");
    // Another font number is another glyph: something must go.
    assert!(reads_other_font(&mut cache, &font, &memory) > 0);
    assert_eq!(reads(&mut cache, '?'), 0, "'?' was used recently and stays");
    assert!(reads(&mut cache, 'A') > 0, "'A' was the oldest and went");
}

fn reads_other_font(cache: &mut GlyphCache, font: &Font, memory: &Memory) -> usize {
    let before = memory.reads.get();
    cache.glyph(1, font, 'é', 12.0);
    memory.reads.get() - before
}

#[test]
fn a_glyph_larger_than_the_cache_is_still_drawn() {
    let pack = open(PACK).unwrap();
    let font = Font::Stored(pack.clone());
    let mut cache = GlyphCache::new(1);
    let glyph = cache.glyph(0, &font, 'A', 24.5);
    assert_eq!((glyph.width, glyph.height), (4, 3));
    assert!(cache.bytes() >= std::mem::size_of::<Glyph>());
    // The next one replaces it.
    cache.glyph(0, &font, '?', 24.5);
    assert_eq!(cache.bytes(), pack.render('?', 24.5).unwrap().byte_size());
}

#[test]
fn packs_read_in_place_bypass_the_cache() {
    let font = Font::Bitmap(BitmapFont::new(PACK).unwrap());
    let mut cache = GlyphCache::new(1 << 20);
    for ch in ['A', '?', 'é', 'Ж'] {
        let _ = cache.glyph(0, &font, ch, 12.0);
    }
    assert_eq!(cache.bytes(), 0);
}
