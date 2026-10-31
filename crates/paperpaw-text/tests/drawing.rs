//! Drawing and measuring text with a bitmap pack (the version 1 test
//! pack): glyphs land where their offsets say, with their coverage, and
//! nothing else changes; bolder text widens glyphs without moving the
//! pen; text at and beyond the canvas edges is clipped. Also what `Font`
//! does for each kind of font, and the character sets packs are made of.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat};
use paperpaw_text::charset;
use paperpaw_text::{
    BitmapFont, Font, FontError, GlyphCache, PackSource, StoredPack, TextStyle, draw_text, glyph_x,
    is_mark, measure,
};

const PACK: &[u8] = include_bytes!("../testdata/pack.ppfp.v1");

fn pack() -> Font {
    Font::from_data(PACK, &[]).unwrap()
}

/// Gray levels of a `width` by `height` canvas after `draw`.
fn draw(
    width: u16,
    height: u16,
    draw: impl FnOnce(&mut paperpaw_graphics::Canvas<'_>),
) -> Vec<Vec<u8>> {
    let mut frame = Framebuffer::new(width, height, PixelFormat::Gray8).unwrap();
    {
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        draw(&mut canvas);
    }
    let view = frame.view();
    (0..height)
        .map(|y| (0..width).map(|x| view.pixel(x, y).r).collect())
        .collect()
}

/// Ink (255 - gray) of each pixel, which is the glyph's coverage for
/// black text.
fn ink(gray: &[Vec<u8>]) -> Vec<Vec<u8>> {
    gray.iter()
        .map(|row| row.iter().map(|g| 255 - g).collect())
        .collect()
}

fn inked(gray: &[Vec<u8>]) -> Vec<(usize, usize, u8)> {
    let mut out = Vec::new();
    for (y, row) in ink(gray).iter().enumerate() {
        for (x, &value) in row.iter().enumerate() {
            if value > 0 {
                out.push((x, y, value));
            }
        }
    }
    out
}

#[test]
fn glyphs_land_where_their_offsets_say() {
    let font = pack();
    let mut cache = GlyphCache::new(1024);
    let style = TextStyle::new(&font, 0, 12.0, Color::BLACK);
    let mut advance = 0.0;
    let gray = draw(40, 24, |canvas| {
        advance = draw_text(canvas, &mut cache, &style, 10.0, 20, "AA");
    });
    assert_eq!(advance, 15.0);
    // 'A': left 0, top -9, 3 by 2: [255, 136, 51], [0, 255, 255]; the
    // second one at 10 + 7.5, rounded to 18.
    let expected: Vec<(usize, usize, u8)> = [10, 18]
        .into_iter()
        .flat_map(|x| {
            [
                (x, 11, 255),
                (x + 1, 11, 136),
                (x + 2, 11, 51),
                (x + 1, 12, 255),
                (x + 2, 12, 255),
            ]
        })
        .collect();
    let mut got = inked(&gray);
    got.sort_by_key(|&(x, y, _)| (y, x));
    let mut expected = expected;
    expected.sort_by_key(|&(x, y, _)| (y, x));
    // Gray8 keeps the coverage up to rounding.
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (g, e) in got.iter().zip(&expected) {
        assert_eq!((g.0, g.1), (e.0, e.1));
        assert!(g.2.abs_diff(e.2) <= 2, "{g:?} is not {e:?}");
    }
    assert_eq!(cache.bytes(), 0, "packs are read in place");
}

/// A combining mark reaches back onto the character before it; with
/// none there, it sits on the pen instead of left of where its text
/// begins.
#[test]
fn a_mark_with_nothing_before_it_starts_at_the_pen() {
    assert!(is_mark('\u{324}') && is_mark('\u{301}') && !is_mark('a') && !is_mark(' '));
    assert_eq!(glyph_x(10.0, -4, false), 6, "on the letter before it");
    assert_eq!(glyph_x(10.0, -4, true), 10, "on nothing");
    assert_eq!(glyph_x(10.0, 2, true), 12, "already right of the pen");
}

#[test]
fn bolder_text_widens_glyphs_but_not_advances() {
    let font = pack();
    let mut cache = GlyphCache::new(1024);
    let style = TextStyle::new(&font, 0, 12.0, Color::BLACK).bolder(2);
    let mut advance = 0.0;
    let gray = draw(30, 16, |canvas| {
        advance = draw_text(canvas, &mut cache, &style, 4.0, 12, "A");
    });
    assert_eq!(advance, 7.5);
    let ink = ink(&gray);
    // Each pixel takes the strongest of itself and the two to its left.
    let row = &ink[3][4..10];
    let expected = [255, 255, 255, 136, 51, 0];
    for (g, e) in row.iter().zip(expected) {
        assert!(g.abs_diff(e) <= 2, "{row:?}");
    }
    let row = &ink[4][4..10];
    let expected = [0, 255, 255, 255, 255, 0];
    for (g, e) in row.iter().zip(expected) {
        assert!(g.abs_diff(e) <= 2, "{row:?}");
    }
}

#[test]
fn text_at_and_beyond_the_edges_is_clipped() {
    let font = pack();
    let mut cache = GlyphCache::new(1024);
    for bolder in [0, 3] {
        let style = TextStyle::new(&font, 0, 24.5, Color::BLACK).bolder(bolder);
        for (x, baseline) in [
            (-2.0, 5),
            (-1000.0, 5),
            (6.0, 3),
            (6.0, 30),
            (1.0e9, 10),
            (-1.0e9, 10),
            (f32::NAN, 10),
            (0.0, i32::MIN / 2),
            (0.0, i32::MAX / 2),
        ] {
            let gray = draw(8, 8, |canvas| {
                draw_text(canvas, &mut cache, &style, x, baseline, "A?é");
            });
            assert_eq!(gray.len(), 8);
        }
    }
    // Wholly outside: nothing drawn.
    let style = TextStyle::new(&font, 0, 12.0, Color::BLACK);
    let gray = draw(8, 8, |canvas| {
        draw_text(canvas, &mut cache, &style, 100.0, 4, "AAA");
        draw_text(canvas, &mut cache, &style, 0.0, 100, "AAA");
    });
    assert!(inked(&gray).is_empty());
}

#[test]
fn measuring_adds_the_advances() {
    let font = pack();
    assert_eq!(measure(&font, 12.0, ""), 0.0);
    assert_eq!(measure(&font, 12.0, "A?é"), 7.5 + 6.0 + 7.0);
    // Missing characters measure as '?'.
    assert_eq!(measure(&font, 12.0, "Ж"), 6.0);
    assert_eq!(measure(&font, 24.5, "AA"), 30.0);
}

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
fn every_kind_of_font_answers_alike() {
    let in_place = pack();
    assert!(matches!(in_place, Font::Bitmap(_)));
    let stored = Font::Stored(StoredPack::open(Rc::new(Memory(PACK.to_vec()))).unwrap());
    for font in [&in_place, &stored] {
        assert_eq!(font.line_metrics(12.0).line_height, 14.25);
        assert_eq!(font.line_metrics(24.0).ascent, 19.0);
        assert_eq!(font.advance('A', 12.0), 7.5);
        assert!(font.has_char('é') && !font.has_char('Ж'));
    }
    // Stored packs say 0 for what they lack, packs in place measure '?'.
    assert_eq!(stored.advance('Ж', 12.0), 0.0);
    assert_eq!(in_place.advance('Ж', 12.0), 6.0);
    let font = BitmapFont::new(PACK).unwrap();
    assert_eq!(font.advance('A', 24.0), 15.0);
}

#[test]
fn font_files_need_outlines_where_they_are_drawn() {
    let data = include_bytes!("../../../plugins/fonts-default/fonts/PaperPawIcons.ttf");
    let font = Font::from_data(&data[..], &[]);
    if paperpaw_text::OUTLINES {
        assert!(font.is_ok());
    } else {
        assert_eq!(font.unwrap_err(), FontError::OutlinesNotSupported);
    }
    assert!(Font::from_data(&b"not a font"[..], &[]).is_err());
    // Errors say what went wrong, each its own way.
    let messages: Vec<String> = [
        FontError::Invalid,
        FontError::NoCharmap,
        FontError::UnsupportedVersion,
        FontError::OutlinesNotSupported,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    for (i, message) in messages.iter().enumerate() {
        assert!(!message.is_empty());
        assert!(!messages[..i].contains(message), "{message}");
    }
}

#[test]
fn character_sets_by_name_and_code_point() {
    assert!(charset::named("latin").is_some_and(|set| set.iter().any(|r| r.contains(&'ſ'))));
    assert!(charset::named("cyrillic").is_some_and(|set| set.iter().any(|r| r.contains(&'Ж'))));
    assert!(charset::named("greek").is_some_and(|set| set.iter().any(|r| r.contains(&'λ'))));
    // The apostrophe of Greek elisions (#60), and all of Hangul (#61).
    assert!(charset::named("greek").is_some_and(|set| set.iter().any(|r| r.contains(&'\u{2bc}'))));
    assert_eq!(
        charset::named("hangul").map(|set| charset::chars(set).count()),
        Some(11_172)
    );
    assert_eq!(charset::named("Latin"), None);
    assert_eq!(charset::named(""), None);
    let latin: Vec<char> = charset::chars(charset::named("latin").unwrap()).collect();
    for c in ['a', 'Z', 'ä', 'ß', 'ő', '€', '\u{2011}', '\u{fffd}'] {
        assert!(latin.contains(&c), "{c:?}");
    }
    assert!(!latin.contains(&'Ж') && !latin.contains(&'\n'));
    let mut sorted = latin.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, latin, "sorted, each once");
    for (text, expected) in [
        ("u+41", Some('A'..='A')),
        ("U+41-5A", Some('A'..='Z')),
        ("U+10FFFF", Some('\u{10ffff}'..='\u{10ffff}')),
        ("U+110000", None),
        ("U+", None),
        ("U+41-", None),
        ("U+-41", None),
        ("U+ 41", None),
        ("U+41-42-43", None),
        ("41", None),
    ] {
        assert_eq!(charset::code_points(text), expected, "{text}");
    }
}
