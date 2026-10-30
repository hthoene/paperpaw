//! Marks drawn by their font's anchors: Hebrew points and Arabic harakat
//! land on their letters (under, inside, over), a kasra under beh's dot;
//! a pack read a glyph at a time and the font itself draw them where the
//! pack in memory does; and a pack of the seven Arabic sizes draws a size
//! it lacks at its nearest one, marks too.

#![cfg(feature = "outline")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_graphics::Framebuffer;
use paperpaw_lang::{Direction, Lang};
use paperpaw_platform::{Color, PixelFormat};
use paperpaw_text::{
    Around, Font, GlyphCache, PackSource, StoredPack, TextStyle, build_pack, charset, draw_text_in,
};

const HEBREW: &[u8] = include_bytes!("../../../tools/fonts/NotoSerifHebrew.ttf");
const ARABIC: &[u8] = include_bytes!("../../../tools/fonts/NotoNaskhArabic.ttf");
/// The sizes `tools/script-pack.py` makes packs in (as Literata's).
const SIZES: [f32; 7] = [20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0];

fn pack(font: &'static [u8], set: &str, sizes: &[f32]) -> Vec<u8> {
    let outline = paperpaw_text::OutlineFont::new(font, &[]).unwrap();
    let chars = charset::chars(charset::named(set).unwrap()).chain([' ']);
    build_pack(&outline, sizes, chars)
}

/// The pixels with ink after drawing `text` right to left at `size`.
fn inked(font: &Font, size: f32, text: &str) -> Vec<(i32, i32)> {
    let mut frame = Framebuffer::new(200, 120, PixelFormat::Gray8).unwrap();
    {
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        let mut cache = GlyphCache::new(64 * 1024);
        let style = TextStyle::new(font, 0, size, Color::BLACK);
        let around = Around::new(Direction::Rtl, Lang::NONE);
        draw_text_in(&mut canvas, &mut cache, &style, 20.0, 80, text, &around);
    }
    let view = frame.view();
    let mut out = Vec::new();
    for y in 0..120u16 {
        for x in 0..200u16 {
            if view.pixel(x, y) != Color::WHITE {
                out.push((i32::from(x), i32::from(y)));
            }
        }
    }
    out
}

/// What `text` with marks inks that the letters alone do not: the marks.
fn marks(font: &Font, size: f32, letters: &str, text: &str) -> Vec<(i32, i32)> {
    let plain = inked(font, size, letters);
    inked(font, size, text)
        .into_iter()
        .filter(|p| !plain.contains(p))
        .collect()
}

/// Left, top, right, bottom of `points`.
fn bounds(points: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    assert!(!points.is_empty());
    let xs = points.iter().map(|p| p.0);
    let ys = points.iter().map(|p| p.1);
    (
        xs.clone().min().unwrap(),
        ys.clone().min().unwrap(),
        xs.max().unwrap(),
        ys.max().unwrap(),
    )
}

#[test]
fn hebrew_points_are_drawn_on_their_letters() {
    let font = Font::from_data(pack(HEBREW, "hebrew", &[20.0]), &[]).unwrap();
    let bet = bounds(&inked(&font, 20.0, "\u{5d1}"));
    // Qamats under bet, within its width.
    let qamats = bounds(&marks(&font, 20.0, "\u{5d1}", "\u{5d1}\u{5b8}"));
    assert!(
        qamats.0 >= bet.0 && qamats.2 <= bet.2 && qamats.1 > 80,
        "{qamats:?} {bet:?}"
    );
    // Dagesh inside it.
    let dagesh = bounds(&marks(&font, 20.0, "\u{5d1}", "\u{5d1}\u{5bc}"));
    assert!(
        dagesh.0 > bet.0 && dagesh.2 < bet.2 && dagesh.1 > bet.1 && dagesh.3 < bet.3,
        "{dagesh:?} {bet:?}"
    );
    // Shin's dot over its right stroke; sin's over its left.
    let shin = bounds(&inked(&font, 20.0, "\u{5e9}"));
    let right = bounds(&marks(&font, 20.0, "\u{5e9}", "\u{5e9}\u{5c1}"));
    let left = bounds(&marks(&font, 20.0, "\u{5e9}", "\u{5e9}\u{5c2}"));
    assert!(
        right.0 > (shin.0 + shin.2) / 2 && right.3 < shin.1 + 2,
        "{right:?} {shin:?}"
    );
    assert!(left.2 <= (shin.0 + shin.2) / 2 + 1, "{left:?} {shin:?}");
}

#[test]
fn a_kasra_goes_under_the_dot_of_beh_a_fatha_over_the_letter() {
    let font = Font::from_data(pack(ARABIC, "arabic", &[20.0]), &[]).unwrap();
    for letters in ["\u{628}", "\u{628}\u{62a}"] {
        // Kasra on the first beh: under all of the word's ink there
        // (dots included).
        let word = bounds(&inked(&font, 20.0, letters));
        let with: String = letters
            .chars()
            .take(1)
            .chain(['\u{650}'])
            .chain(letters.chars().skip(1))
            .collect();
        let kasra = bounds(&marks(&font, 20.0, letters, &with));
        assert!(kasra.1 > word.3 - 1, "{letters}: {kasra:?} {word:?}");
        let with: String = letters
            .chars()
            .take(1)
            .chain(['\u{64e}'])
            .chain(letters.chars().skip(1))
            .collect();
        let fatha = bounds(&marks(&font, 20.0, letters, &with));
        assert!(fatha.3 < 80 - 8, "{letters}: {fatha:?}");
    }
}

/// A pack in memory read through [`PackSource`], as from a card.
struct Card(Vec<u8>);

impl PackSource for Card {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let Some(bytes) = usize::try_from(at)
            .ok()
            .and_then(|at| self.0.get(at..at + buf.len()))
        else {
            return false;
        };
        buf.copy_from_slice(bytes);
        true
    }
}

#[test]
fn packs_on_a_card_and_fonts_place_marks_as_packs_in_memory() {
    for (data, set, text) in [
        (
            HEBREW,
            "hebrew",
            "\u{5e9}\u{5c1}\u{5b8}\u{5dc}\u{5d5}\u{5b9}\u{5dd}",
        ),
        (
            ARABIC,
            "arabic",
            "\u{628}\u{650}\u{633}\u{652}\u{645}\u{650} \u{627}\u{644}\u{644}\u{651}\u{64e}\u{647}\u{650}",
        ),
    ] {
        let bytes = pack(data, set, &[20.0]);
        let memory = Font::from_data(bytes.clone(), &[]).unwrap();
        let card = Font::Stored(StoredPack::open(Rc::new(Card(bytes))).unwrap());
        let drawn = inked(&memory, 20.0, text);
        assert_eq!(inked(&card, 20.0, text), drawn, "{set}");
        let outline = Font::from_data(data, &[]).unwrap();
        assert_eq!(inked(&outline, 20.0, text), drawn, "{set}");
    }
}

#[test]
fn a_size_the_pack_lacks_is_drawn_at_its_nearest_marks_too() {
    // The reader's sizes run past the pack's: a size between two of its
    // strikes is drawn at the nearer (at the smaller where both are as
    // near), one past its largest at the largest.
    let font = Font::from_data(pack(ARABIC, "arabic", &SIZES), &[]).unwrap();
    let text = "\u{628}\u{650}\u{633}\u{652}\u{645}\u{64e}";
    for size in [21.0, 26.0, 30.0, 34.0, 38.0, 44.0, 46.0, 52.0] {
        let nearest = SIZES
            .iter()
            .copied()
            .min_by(|a, b| (a - size).abs().total_cmp(&(b - size).abs()))
            .unwrap();
        assert_eq!(
            inked(&font, size, text),
            inked(&font, nearest, text),
            "{size}"
        );
    }
    assert_ne!(inked(&font, 44.0, text), inked(&font, 48.0, text));
}

/// Packs of Latin, Cyrillic and Greek get no anchors table: they are made
/// byte for byte as before there were anchors (the glyphs and kerning of
/// `build_pack` alone, version 1, no table directory), so the firmware's
/// packs and the store's Literata packs stay as they are.
#[test]
fn packs_of_other_scripts_are_made_as_before() {
    use paperpaw_text::{BitmapFont, OutlineFont, PackBuilder};

    const LITERATA: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/Literata.ttf");
    let outline = OutlineFont::new(LITERATA, &[("wght", 400.0)]).unwrap();
    let sizes = [20.0, 32.0];
    for sets in [&["latin", "ligatures"][..], &["cyrillic"], &["greek"]] {
        let chars: Vec<char> = sets
            .iter()
            .flat_map(|s| charset::chars(charset::named(s).unwrap()))
            .collect();
        let pack = build_pack(&outline, &sizes, chars.iter().copied());
        assert_eq!(pack[4], 1, "{sets:?}: version 1");
        assert_eq!(
            BitmapFont::new(pack.clone())
                .unwrap()
                .table(BitmapFont::ANCHORS),
            None
        );
        // What build_pack made before anchors.
        let mut builder = PackBuilder::new();
        builder.kerning(&outline.kerning_pairs(&chars));
        for &size in &sizes {
            let glyphs = chars
                .iter()
                .filter(|&&ch| outline.has_char(ch))
                .map(|&ch| (ch, outline.rasterize(ch, size)))
                .collect();
            builder.add_strike(size, outline.line_metrics(size), glyphs);
        }
        assert!(builder.build() == pack, "{sets:?}: the same bytes");
    }
}

/// Of two sizes as near, the smaller, also where a pack holds its sizes
/// out of order: in memory and on a card alike.
#[test]
fn of_two_sizes_as_near_the_smaller_whatever_their_order() {
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

    let mut builder = PackBuilder::new();
    for (size, width) in [(24.0, 3.0), (20.0, 2.0)] {
        builder.add_strike(
            size,
            LineMetrics::new(size, 2.0, size),
            vec![('a', Glyph::empty(width))],
        );
    }
    let bytes = builder.build();
    let memory = Font::from_data(bytes.clone(), &[]).unwrap();
    let card = Font::Stored(StoredPack::open(Rc::new(Card(bytes))).unwrap());
    for font in [&memory, &card] {
        assert_eq!(font.advance('a', 22.0), 2.0);
        assert_eq!(font.advance('a', 23.0), 3.0);
        assert_eq!(font.line_metrics(22.0).ascent, 20.0);
    }
}
