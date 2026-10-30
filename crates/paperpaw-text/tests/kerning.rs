//! Kerning read from the default fonts and kept in packs: well-known
//! pairs come out closer, a pack kerns as its font does, and packs
//! without a kerning table (made before there was one) read as before.

#![cfg(feature = "outline")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_text::charset::{self, LATIN};
use paperpaw_text::{
    BitmapFont, Font, LineMetrics, OutlineFont, PackBuilder, PackSource, StoredPack, build_pack,
    measure,
};

const LITERATA: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/Literata.ttf");
const LITERATA_ITALIC: &[u8] =
    include_bytes!("../../../plugins/fonts-default/fonts/Literata-Italic.ttf");
const SOURCE_SANS: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/SourceSans3.ttf");
const ICONS: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/PaperPawIcons.ttf");
const PACK_V1: &[u8] = include_bytes!("../testdata/pack.ppfp.v1");

fn pair(pairs: &[(char, char, f32)], left: char, right: char) -> Option<f32> {
    pairs
        .iter()
        .find(|p| p.0 == left && p.1 == right)
        .map(|p| p.2)
}

#[test]
fn well_known_pairs_come_out_closer() {
    let latin: Vec<char> = charset::chars(LATIN).collect();
    for (data, weight) in [
        (LITERATA, 400.0),
        (LITERATA, 700.0),
        (LITERATA_ITALIC, 400.0),
        (SOURCE_SANS, 430.0),
    ] {
        let font = OutlineFont::new(data, &[("wght", weight)]).unwrap();
        let pairs = font.kerning_pairs(&latin);
        for (l, r) in [('A', 'V'), ('T', 'o'), ('L', 'T'), ('V', 'A')] {
            let em = pair(&pairs, l, r).unwrap_or(0.0);
            assert!(em < -0.01 && em > -0.3, "{l}{r} at {weight}: {em}");
            // At runtime the same, in pixels.
            let px = font.kern(l, r, 100.0);
            assert!((px - em * 100.0).abs() < 1e-3, "{l}{r}: {px} or {em}");
        }
        // The accented forms of a letter kern as it does.
        assert_eq!(pair(&pairs, '\u{c1}', 'V'), pair(&pairs, 'A', 'V'));
        // Nothing for pairs that change nothing.
        assert!(pairs.iter().all(|p| p.2 != 0.0));
        assert_eq!(font.kern('A', '\u{4e00}', 24.0), 0.0);
    }
    // The bold instance of the variable font kerns as its own.
    let regular = OutlineFont::new(LITERATA, &[("wght", 400.0)]).unwrap();
    let bold = OutlineFont::new(LITERATA, &[("wght", 700.0)]).unwrap();
    assert_ne!(regular.kern('A', 'V', 100.0), bold.kern('A', 'V', 100.0));
}

#[test]
fn fonts_without_kerning_have_no_pairs() {
    let font = OutlineFont::new(ICONS, &[]).unwrap();
    let icons: Vec<char> = ('\u{e000}'..='\u{e100}').collect();
    assert!(font.kerning_pairs(&icons).is_empty());
}

#[test]
fn packs_kern_as_their_font_does() {
    let font = OutlineFont::new(LITERATA, &[("wght", 400.0)]).unwrap();
    let chars = "AVTWoaey.,\u{c0}\u{e9}";
    let pack = Font::from_data(build_pack(&font, &[24.0, 48.0], chars.chars()), &[]).unwrap();
    let outline = Font::Outline(font);
    for l in chars.chars() {
        for r in chars.chars() {
            for size in [24.0, 48.0] {
                let (packed, runtime) = (pack.kern(l, r, size), outline.kern(l, r, size));
                // Kept in 1/1024 em.
                assert!((packed - runtime).abs() <= size / 2048.0, "{l}{r}");
            }
        }
        // Characters outside the pack are not kerned.
        assert_eq!(pack.kern(l, 'Z', 24.0), 0.0);
    }
    // At the nearest size of the pack.
    assert_eq!(pack.kern('A', 'V', 25.0), pack.kern('A', 'V', 24.0));
    // Measuring kerns the pairs of the text.
    let apart = pack.advance('A', 24.0) + pack.advance('V', 24.0);
    let kerned = measure(&pack, 24.0, "AV");
    assert!((kerned - (apart + pack.kern('A', 'V', 24.0))).abs() < 1e-4);
    assert!(kerned < apart - 1.0);
}

/// A pack in memory, read as one on a card.
struct Memory(Vec<u8>);

impl PackSource for Memory {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
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

#[test]
fn packs_with_and_without_kerning_read_alike() {
    // Made before kerning: no table, nothing kerned.
    let old = Font::from_data(PACK_V1, &[]).unwrap();
    assert_eq!(old.kern('A', '?', 12.0), 0.0);
    assert_eq!(measure(&old, 12.0, "A?"), 7.5 + 6.0);

    // The same glyphs with a table: the glyphs read as before, and a
    // pack read a glyph at a time reads it too (without kerning).
    let glyphs = |builder: &mut PackBuilder| {
        let metrics = LineMetrics::new(8.0, 2.0, 12.0);
        let glyph = |advance| paperpaw_text::Glyph::empty(advance);
        builder.add_strike(10.0, metrics, vec![('A', glyph(6.0)), ('V', glyph(6.0))]);
        builder.add_strike(20.0, metrics, vec![('A', glyph(12.0)), ('V', glyph(12.0))]);
    };
    let mut plain = PackBuilder::new();
    glyphs(&mut plain);
    let mut kerned = PackBuilder::new();
    glyphs(&mut kerned);
    kerned.kerning(&[('A', 'V', -0.125), ('V', 'A', -0.0625)]);
    let (plain, kerned) = (plain.build(), kerned.build());
    // The table follows everything else, which stays as it was but for
    // the field that points to it.
    assert_eq!(kerned[..40], plain[..40]);
    assert_eq!(kerned[44..plain.len()], plain[44..]);
    let font = BitmapFont::new(kerned.clone()).unwrap();
    assert_eq!(font.kern('A', 'V', 10.0), -1.25);
    assert_eq!(font.kern('A', 'V', 20.0), -2.5);
    assert_eq!(font.kern('V', 'A', 20.0), -1.25);
    assert_eq!(font.kern('V', 'V', 20.0), 0.0);
    assert_eq!(font.advance('V', 20.0), 12.0);
    let stored = StoredPack::open(Rc::new(Memory(kerned))).unwrap();
    let stored = Font::Stored(stored);
    assert_eq!(stored.advance('A', 10.0), 6.0);
    assert_eq!(stored.kern('A', 'V', 10.0), 0.0);
}
