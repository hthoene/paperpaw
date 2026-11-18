//! The interface's text in Arabic (#33): measured, drawn and wrapped as
//! the reader sets it, Arabic letters joined, without being asked; a
//! line of both directions starting at the right where the handle's ink
//! says its lines run right to left (the interface's font in Arabic),
//! else as its first strong letter goes. Text left to right alone is
//! drawn as before.

use super::*;
use crate::lang::{Direction, Lang};
use paperpaw_text::{Around, TextInk, ToneCurve};

/// The ink of the interface's font in Arabic.
fn rtl() -> TextInk {
    TextInk::DEFAULT.with_flags(TextInk::RIGHT_TO_LEFT)
}

/// Three behs: initial, medial and final.
const BEHS: &str = "\u{628}\u{628}\u{628}";

/// What `font.draw` draws of `text`, and the width it returns.
fn drawn_by(font: &FontHandle<'_>, text: &str) -> (alloc::vec::Vec<u8>, f32) {
    let mut width = 0.0;
    let frame = drawn(|c| width = font.draw(c, 20.0, 4.0, 30, text, Color::BLACK));
    (frame, width)
}

/// What `draw_in` draws of `text` with `around`.
fn drawn_in(font: &FontHandle<'_>, text: &str, around: &Around) -> alloc::vec::Vec<u8> {
    let tone = ToneCurve::SMOOTH;
    drawn(|c| {
        font.draw_in(c, 20.0, 4.0, 30, text, Color::BLACK, (0, tone), around);
    })
}

#[test]
fn arabic_is_measured_joined_without_being_asked() {
    let (fonts, _storage) = latin_and_letters("");
    let font = fonts.get("a.latin").unwrap();
    // Initial 2, medial 3, final 4; alone 1.
    assert_eq!(font.measure(10.0, BEHS), 2.0 + 3.0 + 4.0);
    assert_eq!(font.measure(10.0, "\u{628}"), 1.0);
    assert_eq!(font.measure(10.0, "\u{628} \u{628}"), 1.0 + 5.0 + 1.0);
    assert_eq!(
        font.measure(10.0, BEHS),
        font.measure_in(10.0, BEHS, &Around::NONE)
    );
    // Beside Latin letters, in parts: 5 + 5 + 5, then 2 + 4.
    assert_eq!(font.measure(10.0, "ab \u{628}\u{628}"), 21.0);
    // As wide on a line right to left.
    let ui = font.inked(rtl());
    for text in [BEHS, "ab \u{628}\u{628}", "\u{628}\u{628} (ab)", "abc"] {
        assert_eq!(ui.measure(10.0, text), font.measure(10.0, text), "{text}");
    }
}

#[test]
fn arabic_is_drawn_joined_as_measured() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    let (joined, width) = drawn_by(&font, BEHS);
    assert_eq!(joined, drawn_in(&font, BEHS, &Around::NONE));
    assert_eq!(width, font.measure(20.0, BEHS));
    // Not the letters alone, one beside the other.
    let alone = drawn(|c| {
        let mut x = 4.0;
        for _ in 0..3 {
            x += font.draw(c, 20.0, x, 30, "\u{628}", Color::BLACK);
        }
    });
    assert_ne!(joined, alone);
    // Toned, bolder: the same way.
    let tone = ToneCurve::contrast(2);
    let toned = drawn(|c| {
        font.draw_toned(c, 20.0, 4.0, 30, BEHS, Color::BLACK, 1, tone);
    });
    let shaped = drawn(|c| {
        font.draw_in(
            c,
            20.0,
            4.0,
            30,
            BEHS,
            Color::BLACK,
            (1, tone),
            &Around::NONE,
        );
    });
    assert_eq!(toned, shaped);
}

/// `Wi-Fi متصل` in an Arabic interface: `Wi-Fi` at the right, read
/// first; with any other font (a book's title in the reader's), or in a
/// language left to right, `Wi-Fi` at the left, as its first letter
/// goes.
#[test]
fn a_line_of_both_directions_starts_at_the_right_in_an_arabic_interface() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    let ui = font.inked(rtl());
    let text = "aa \u{628}\u{628}";
    let right_to_left = Around::new(Direction::Rtl, Lang::NONE);
    let left_to_right = Around::new(Direction::Ltr, Lang::NONE);
    let (from_right, width) = drawn_by(&ui, text);
    assert_eq!(from_right, drawn_in(&font, text, &right_to_left));
    assert_eq!(width, ui.measure(20.0, text));
    let (from_left, _) = drawn_by(&font, text);
    assert_eq!(from_left, drawn_in(&font, text, &left_to_right));
    assert_ne!(from_right, from_left);
    // Arabic first: right to left either way.
    let text = "\u{628}\u{628} aa";
    assert_eq!(drawn_by(&ui, text).0, drawn_by(&font, text).0);
    assert_eq!(
        drawn_by(&font, text).0,
        drawn_in(&font, text, &right_to_left)
    );
}

#[test]
fn text_left_to_right_alone_is_drawn_as_before_in_an_arabic_interface() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    let ui = font.inked(rtl());
    for text in ["a(a): a.", "(a)", "", " "] {
        assert_eq!(drawn_by(&ui, text), drawn_by(&font, text), "{text:?}");
        assert_eq!(ui.wrap(20.0, text, 30.0), font.wrap(20.0, text, 30.0));
    }
    // Arabic alone, too: one way only.
    for text in [BEHS, "\u{628} \u{628}\u{628}"] {
        assert_eq!(drawn_by(&ui, text), drawn_by(&font, text), "{text:?}");
    }
}

/// Lines of Arabic words fit as drawn, joined, not as their letters
/// alone (which are narrower here, so lines would run over).
#[test]
fn arabic_is_wrapped_as_wide_as_drawn() {
    let (fonts, _storage) = latin_and_letters("");
    let font = fonts.get("a.latin").unwrap();
    // Each word 2 + 3 + 3 + 4 = 12 wide, a space 5.
    let word = "\u{628}\u{628}\u{628}\u{628}";
    let text = alloc::format!("{word} {word} {word}");
    assert_eq!(font.measure(10.0, word), 12.0);
    for ui in [font, font.inked(rtl())] {
        let lines = ui.wrap(10.0, &text, 30.0);
        assert_eq!(lines.len(), 2, "{lines:?}");
        for line in &lines {
            let shown = text[line.clone()].trim();
            assert!(ui.measure(10.0, shown) <= 30.0, "{shown}");
        }
    }
    // Beside Latin: the parts' widths.
    let mixed = alloc::format!("ab {word} ab");
    let lines = font.wrap(10.0, &mixed, 22.0);
    for line in &lines {
        let shown = mixed[line.clone()].trim();
        assert!(font.measure(10.0, shown) <= 22.0, "{shown}");
    }
}

/// Lam and alef are drawn as one letter, lam-alef, whose width is not
/// theirs together (wider here): lines with it fit as drawn.
#[test]
fn lam_alef_is_wrapped_as_wide_as_drawn() {
    let (fonts, _storage) = latin_and_letters("");
    let font = fonts.get("a.latin").unwrap();
    // Alone and after a beh, which joins it: the ligature, isolated and
    // final, 7 wide.
    assert_eq!(font.measure(10.0, "\u{644}\u{627}"), 7.0);
    assert_eq!(font.measure(10.0, "\u{628}\u{644}\u{627}"), 2.0 + 7.0);
    let word = "\u{628}\u{644}\u{627}\u{628}\u{644}\u{627}";
    let text = alloc::format!("{word} {word} {word} \u{644}\u{627} {word}");
    for ui in [font, font.inked(rtl())] {
        // A word 2 + 7 + 2 + 7 wide, a space 5.
        for width in [18.0, 23.0, 30.0, 41.0] {
            let lines = ui.wrap(10.0, &text, width);
            assert!(lines.len() > 1, "{width}");
            for line in &lines {
                let shown = text[line.clone()].trim();
                assert!(ui.measure(10.0, shown) <= width, "{width}: {shown}");
            }
        }
    }
}

/// What no font has, marks, controls and odd mixes: measured and drawn
/// without a panic, the width drawn as measured.
#[test]
fn odd_text_of_both_directions_is_drawn_as_measured() {
    let (fonts, _storage) = inked_fonts();
    let font = fonts.get("a.latin").unwrap();
    let ui = font.inked(rtl());
    for text in [
        "\u{64b}",
        "\u{628}\u{64b}\u{628}",
        "\u{202e}aa\u{202c}",
        "\u{200f}a",
        "a\u{5d0}\u{628}1(2)",
        "\u{628}\u{200d}",
        "\u{fffd}\u{628}\0",
        "\u{628}\n\u{628}",
    ] {
        for handle in [&font, &ui] {
            let (_, width) = drawn_by(handle, text);
            assert_eq!(width, handle.measure(20.0, text), "{text:?}");
            let _ = handle.wrap(20.0, text, 10.0);
        }
    }
}
