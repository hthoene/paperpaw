//! Native replacements of the typing tests of the interface-6 suite
//! `typing.rs`: letters, capitals, symbols, digits, delete, the longest text,
//! variants, how the buttons move the focus, and layout keys, on a
//! [`Board`](crate::native_model::Board) driven by taps (a point on a key,
//! found by `key_at`) and by the button presses of the plugin's dialog
//! (`move_focus`, then the focused key pressed). What the dialog and the
//! host do around it is in `tests/acceptance_pending.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::keys::Layer;
use crate::native_model::{Asked, Board, FieldButton, MAX_CHARS, Move, Pressed, Shift};
use crate::native_model_tests::{
    BOOX, Typing, X4, X4_PRO, board_with, find, label, layouts, look, own, plain,
};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The letters of the English layout, row after row.
const ENGLISH: &str = "qwertyuiopasdfghjklzxcvbnm";
const TWO_LAYOUTS: &str = "keyboard.layout.en, keyboard.layout.de";

fn number() -> Asked {
    Asked {
        number: true,
        ..Asked::default()
    }
}

fn secret() -> Asked {
    Asked {
        secret: true,
        ..Asked::default()
    }
}

#[test]
fn every_letter_typed_by_buttons_gives_the_text() {
    let mut typing = Typing::new(X4, plain());
    // The focus starts on the first key, as buttons need one.
    assert_eq!(typing.focused().as_deref(), Some("q"));
    for c in ENGLISH.chars() {
        typing.press_by_buttons(&c.to_string());
    }
    assert_eq!(typing.press_by_buttons("OK"), Pressed::Done);
    assert_eq!(typing.text(), ENGLISH);
}

/// Typed with buttons a letter at a time, a text is quicker on a phone: where
/// one can be, the focus starts on *Phone*. Touched, nothing is focused.
#[test]
fn with_buttons_alone_the_focus_starts_on_phone() {
    let phone = [(FieldButton::Phone, 40.0)];
    let mut typing = Typing::of(board_with(X4, plain(), "keyboard.layout.auto", &phone));
    assert_eq!(typing.focused().as_deref(), Some("Phone"));
    // The keys are a press of Next away.
    typing.press_by_buttons("a");
    assert_eq!(typing.text(), "a");
    let touched = Typing::of(board_with(X4_PRO, plain(), "keyboard.layout.auto", &phone));
    assert_eq!(touched.board.buttons.len(), 1);
    assert_eq!(touched.focused(), None);
}

/// Right from the last button of the field (*Phone*) goes straight to the
/// first key; left and the other moves keep their ways.
#[test]
fn right_from_phone_goes_to_the_first_key() {
    let phone = [(FieldButton::Phone, 40.0)];
    let mut typing = Typing::of(board_with(X4, plain(), "keyboard.layout.auto", &phone));
    let go = |typing: &mut Typing, to: Move| {
        typing.board.move_focus(to);
        typing.focused()
    };
    assert_eq!(typing.focused().as_deref(), Some("Phone"));
    let first = go(&mut typing, Move::Next);
    assert_ne!(first.as_deref(), Some("Phone"));
    // Back up to Phone, then Right.
    assert_eq!(go(&mut typing, Move::Up).as_deref(), Some("Phone"));
    assert_eq!(go(&mut typing, Move::Right), first);
    // Left from the first key goes round its row, not back up to Phone.
    let left = go(&mut typing, Move::Left);
    assert_ne!(left, first);
    assert_ne!(left.as_deref(), Some("Phone"));
    // Up from the first row is Phone again; Left there stays on it.
    assert_eq!(go(&mut typing, Move::Up).as_deref(), Some("Phone"));
    assert_eq!(go(&mut typing, Move::Left).as_deref(), Some("Phone"));
    // Down goes to the key below Phone.
    assert_ne!(go(&mut typing, Move::Down).as_deref(), Some("Phone"));
}

#[test]
fn every_letter_typed_by_touch_gives_the_text() {
    let mut typing = Typing::new(X4_PRO, plain());
    // No focus while tapping.
    assert_eq!(typing.focused(), None);
    typing.tap_all(ENGLISH);
    assert_eq!(typing.tap("OK"), Pressed::Done);
    assert_eq!(typing.text(), ENGLISH);
}

#[test]
fn shift_makes_one_capital() {
    let mut typing = Typing::new(X4_PRO, plain());
    typing.tap("Shift");
    // The keys show capitals now.
    assert!(find(&typing.board, "H").is_some());
    assert!(find(&typing.board, "h").is_none());
    typing.tap("H");
    // Only the next letter.
    typing.tap("i");
    // Pressed, then again a while later: none.
    typing.tap("Shift");
    typing.now += 2000;
    typing.tap("Shift");
    typing.tap("x");
    assert_eq!(typing.text(), "Hix");
}

/// *Shift* twice in a row locks capitals until it is pressed again.
#[test]
fn shift_twice_locks_capitals() {
    let mut typing = Typing::new(X4, plain());
    typing.press_by_buttons("Shift");
    typing.confirm();
    assert_eq!(typing.board.shift, Shift::Locked);
    for c in ["O", "K", "A", "Y"] {
        typing.press_by_buttons(c);
    }
    typing.press_by_buttons("Shift");
    typing.press_by_buttons("s");
    assert_eq!(typing.text(), "OKAYs");
}

/// With *Automatic capitals*, a text and each sentence start with one; not
/// where it is off (the default), nor in an address.
#[test]
fn automatic_capitals_start_sentences() {
    let on = |asked| {
        Typing::of(
            Board::new(
                look(X4_PRO),
                own("keyboard.size.medium", true),
                asked,
                layouts("keyboard.layout.auto", false),
                0,
                &[],
            )
            .unwrap(),
        )
    };
    let mut typing = on(plain());
    for key in ["H", "i", ".", "Space", "H", "o"] {
        typing.tap(key);
    }
    assert_eq!(typing.text(), "Hi. Ho");

    // An address (mode url) is not plain text.
    let mut address = on(Asked::default());
    address.tap("w");
    assert_eq!(address.text(), "w");

    let mut off = Typing::new(X4_PRO, plain());
    off.tap("h");
    assert_eq!(off.text(), "h");
}

#[test]
fn symbols_digits_and_space_by_buttons() {
    let mut typing = Typing::new(X4, plain());
    typing.press_by_buttons("a");
    typing.press_by_buttons("Space");
    typing.press_by_buttons("123");
    for c in "1@#9?".chars() {
        typing.press_by_buttons(&c.to_string());
    }
    typing.press_by_buttons("ABC");
    typing.press_by_buttons("b");
    typing.press_by_buttons(",");
    typing.press_by_buttons(".");
    typing.press_by_buttons("OK");
    assert_eq!(typing.text(), "a 1@#9?b,.");
}

#[test]
fn number_mode_has_only_digits() {
    let mut typing = Typing::new(X4_PRO, number());
    assert!(find(&typing.board, "q").is_none());
    assert!(find(&typing.board, "123").is_none());
    typing.tap_all("4072");
    typing.tap("Delete");
    typing.tap("0");
    assert_eq!(typing.tap("OK"), Pressed::Done);
    assert_eq!(typing.text(), "4070");
}

#[test]
fn delete_on_an_empty_field_does_nothing() {
    let mut typing = Typing::new(X4, plain());
    for _ in 0..3 {
        assert_eq!(typing.press_by_buttons("Delete"), Pressed::Nothing);
    }
    typing.press_by_buttons("a");
    assert_eq!(typing.text(), "a");
}

#[test]
fn delete_takes_whole_characters_of_the_initial_text() {
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.text = String::from("Grüße 😀");
    for _ in 0..3 {
        typing.tap("Delete");
    }
    assert_eq!(typing.text(), "Grüß");
}

/// A long press on *Delete* is the board's `clear` (the dialog maps the
/// gesture; the host's part is `keyboard-touch-holds` in the acceptance
/// cases): it starts the text over and typing goes on.
#[test]
fn holding_delete_clears_the_text() {
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.text = String::from("old text");
    typing.board.clear();
    typing.tap("n");
    assert_eq!(typing.text(), "n");
}

#[test]
fn text_stops_at_the_longest_allowed() {
    let mut typing = Typing::new(X4_PRO, plain());
    let mut full = 0;
    for _ in 0..MAX_CHARS + 5 {
        if typing.tap("a") == Pressed::Full {
            full += 1;
        }
    }
    assert_eq!(full, 5);
    assert_eq!(typing.text(), "a".repeat(MAX_CHARS));
}

#[test]
fn an_initial_text_longer_than_allowed_is_kept_but_not_added_to() {
    let long = "x".repeat(MAX_CHARS + 10);
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.text = long.clone();
    assert_eq!(typing.tap("a"), Pressed::Full);
    assert_eq!(typing.text(), long);
}

#[test]
fn variants_by_touch() {
    let mut typing = Typing::new(X4_PRO, plain());
    assert!(typing.hold("e"));
    // The variants show above the key; one is chosen by a tap.
    let popup = typing.board.popup.as_ref().unwrap();
    assert!(popup.chars.contains(&'é') && popup.chars.contains(&'è'));
    typing.choose('é');
    // The variants are gone, the one chosen is in the field.
    assert!(typing.board.popup.is_none());
    assert_eq!(typing.text(), "é");
    typing.tap("Shift");
    assert!(typing.hold("U"));
    typing.choose('Ü');
    assert_eq!(typing.text(), "éÜ");
    // The capital was for that letter only.
    assert_eq!(typing.board.shift, Shift::Off);
}

#[test]
fn variants_by_buttons() {
    let mut typing = Typing::new(X4, plain());
    // To the `e`, then a long press on confirm.
    for _ in 0..2 {
        typing.board.move_focus(Move::Right);
    }
    assert_eq!(typing.focused().as_deref(), Some("e"));
    assert!(typing.hold_confirm());
    // The capital first, then the variants.
    assert_eq!(typing.focused().as_deref(), Some("E"));
    typing.board.variants_move(Move::Right);
    assert_eq!(typing.focused().as_deref(), Some("è"));
    typing.board.variants_move(Move::Right);
    assert_eq!(typing.focused().as_deref(), Some("é"));
    typing.choose('é');
    // Back to the keys: the focus is on the `e` again.
    assert_eq!(typing.focused().as_deref(), Some("e"));
    typing.press_by_buttons("OK");
    assert_eq!(typing.text(), "é");
}

/// A letter at the screen's edge and a sign in the bottom row show their
/// variants, within the screen (whether the host lets a finger held there
/// reach the keyboard is `keyboard-touch-holds` in the acceptance cases).
#[test]
fn holding_a_letter_at_the_edge_shows_its_variants() {
    for screen in [X4_PRO, BOOX] {
        let mut typing = Typing::new(screen, plain());
        for (key, variant) in [("a", 'ä'), (".", '…')] {
            assert!(typing.hold(key), "no variants for {key:?}");
            let popup = typing.board.popup.as_ref().unwrap();
            let area = popup.area;
            assert!(area.x + area.width <= screen.width, "{key} {area:?}");
            assert!(area.y + area.height <= screen.height, "{key} {area:?}");
            typing.choose(variant);
        }
        assert_eq!(typing.text(), "ä…");
    }
}

/// A long press on a letter shows its capital first, chosen: a long press and
/// Select type it, without going to *Shift*.
#[test]
fn a_long_press_on_a_letter_offers_its_capital_first() {
    let mut typing = Typing::new(X4, plain());
    assert_eq!(typing.focused().as_deref(), Some("q"));
    assert!(typing.hold_confirm());
    assert_eq!(typing.focused().as_deref(), Some("Q"));
    typing.choose('Q');
    typing.press_by_buttons("u");
    assert_eq!(typing.text(), "Qu");

    let mut touch = Typing::new(X4_PRO, plain());
    assert!(touch.hold("e"));
    assert_eq!(touch.board.popup.as_ref().unwrap().chars[0], 'E');
    touch.choose('E');
    assert_eq!(touch.text(), "E");
}

/// Typing a password (as for Wi-Fi on the X4), holding Select still shows a
/// letter's capital and variants.
#[test]
fn holding_select_works_while_typing_a_password() {
    let mut typing = Typing::new(X4, secret());
    for _ in 0..2 {
        typing.board.move_focus(Move::Right);
    }
    assert_eq!(typing.focused().as_deref(), Some("e"));
    assert!(typing.hold_confirm());
    assert_eq!(typing.focused().as_deref(), Some("E"));
    typing.board.variants_move(Move::Right);
    assert_eq!(typing.focused().as_deref(), Some("è"));
    typing.choose('è');
    assert_eq!(typing.text(), "è");
    // Off a letter there are no variants: the dialog finishes there.
    typing.focus_by_buttons("OK");
    assert!(!typing.hold_confirm());
}

/// A long press off a letter shows nothing, so the dialog finishes there.
#[test]
fn a_long_press_off_a_letter_has_no_variants() {
    let mut typing = Typing::new(X4, plain());
    typing.focus_by_buttons("Space");
    assert!(!typing.hold_confirm());
    assert!(typing.board.popup.is_none());
}

#[test]
fn buttons_move_round_the_rows() {
    let mut typing = Typing::new(X4, plain());
    let go = |typing: &mut Typing, to: Move| {
        typing.board.move_focus(to);
        typing.focused()
    };
    // The rows a computer keyboard's, also on this narrow screen. Left from
    // the first key wraps to the end of the row.
    assert_eq!(go(&mut typing, Move::Left).as_deref(), Some("p"));
    assert_eq!(go(&mut typing, Move::Right).as_deref(), Some("q"));
    // Down to the row below, near the same place.
    assert_eq!(go(&mut typing, Move::Down).as_deref(), Some("a"));
    // Previous from the first key of a row: the end of the row above.
    assert_eq!(go(&mut typing, Move::Previous).as_deref(), Some("p"));
    // Up from the first row: the bottom row, below (no buttons in the field
    // without a clipboard or network).
    assert_eq!(go(&mut typing, Move::Up).as_deref(), Some("OK"));
    // Right from the end of a row wraps to its start.
    assert_eq!(go(&mut typing, Move::Right).as_deref(), Some("123"));
    // Down from the bottom row: the top row.
    assert_eq!(go(&mut typing, Move::Down).as_deref(), Some("q"));
    // Next from the end of the last row: the first key.
    go(&mut typing, Move::Up);
    assert_eq!(go(&mut typing, Move::Left).as_deref(), Some("OK"));
    assert_eq!(go(&mut typing, Move::Next).as_deref(), Some("q"));
}

/// Up and down keep to the column the focus came along, rows of other widths
/// between: down twice and up twice come back to the same key.
#[test]
fn up_and_down_keep_the_column() {
    let mut typing = Typing::new(X4, plain());
    typing.focus_by_buttons("o");
    for go in [Move::Down, Move::Down, Move::Up, Move::Up] {
        typing.board.move_focus(go);
    }
    assert_eq!(typing.focused().as_deref(), Some("o"));
}

#[test]
fn the_layout_key_switches_by_touch() {
    let mut typing = Typing::of(board_with(X4_PRO, plain(), TWO_LAYOUTS, &[]));
    // The key shows the layout in use.
    assert!(find(&typing.board, "EN").is_some());
    assert!(find(&typing.board, "ü").is_none());
    typing.tap("EN");
    assert!(find(&typing.board, "DE").is_some());
    assert_eq!(typing.board.current, 1);
    // Umlauts and ß by holding u and s.
    assert!(find(&typing.board, "ü").is_none());
    assert!(typing.hold("u"));
    typing.choose('ü');
    assert!(typing.hold("s"));
    typing.choose('ß');
    // `ß` has no single capital: it stays as it is.
    typing.tap("Shift");
    assert!(typing.hold("S"));
    typing.choose('ß');
    assert_eq!(typing.text(), "üßß");
    assert!(find(&typing.board, "z").is_some());
    typing.tap("DE");
    assert!(find(&typing.board, "EN").is_some());
}

#[test]
fn the_layout_key_by_buttons() {
    let mut typing = Typing::of(board_with(X4, plain(), TWO_LAYOUTS, &[]));
    typing.press_by_buttons("EN");
    // On the `o` the field says what holding Select types; it comes after
    // the capital there.
    typing.focus_by_buttons("o");
    assert_eq!(typing.board.held_letters().as_deref(), Some("ö"));
    assert!(typing.hold_confirm());
    // After the capital.
    assert_eq!(typing.focused().as_deref(), Some("O"));
    typing.board.variants_move(Move::Right);
    assert_eq!(typing.focused().as_deref(), Some("ö"));
    typing.choose('ö');
    // Not on a letter without.
    typing.press_by_buttons("y");
    assert!(typing.board.held_letters().is_none());
    assert_eq!(typing.text(), "öy");
}

/// Buttons, then a tap: the hint of the letters held on the key that had the
/// focus goes with the focus (a tap takes the focus away, `focus` is none;
/// the field drawn again is `keyboard-touch-holds`).
#[test]
fn a_tap_takes_the_hint_of_held_letters_with_the_focus() {
    let mut typing = Typing::of(board_with(X4_PRO, plain(), "keyboard.layout.de", &[]));
    let (row, index) = find(&typing.board, "a").unwrap();
    typing.board.focus = Some((row, index));
    assert_eq!(typing.board.held_letters().as_deref(), Some("ä"));
    typing.board.shift = Shift::Once;
    assert_eq!(typing.board.held_letters().as_deref(), Some("Ä"));
    typing.board.focus = None;
    assert!(typing.board.held_letters().is_none());
}

/// German with *Umlaut keys*: ä, ö, ü and ß have keys of their own.
#[test]
fn umlaut_keys_give_german_keys_for_them() {
    for keys in [false, true] {
        let layouts = layouts("keyboard.layout.de", keys);
        let board = Board::new(
            look(X4_PRO),
            crate::native_model_tests::own("keyboard.size.medium", false),
            plain(),
            layouts,
            0,
            &[],
        )
        .unwrap();
        for letter in ["ä", "ö", "ü", "ß"] {
            assert_eq!(find(&board, letter).is_some(), keys, "{letter}");
        }
    }
}

/// What a font without Cyrillic and Greek draws.
fn latin_only(text: &str) -> bool {
    text.chars().all(|c| !matches!(c, '\u{370}'..='\u{4FF}'))
}

/// The letters one character long that a board shows.
fn letters(board: &Board) -> Vec<String> {
    board
        .rows
        .iter()
        .flatten()
        .map(|(key, _)| label(board, *key))
        .filter(|l| l.chars().count() == 1 && l.chars().all(char::is_alphabetic))
        .collect()
}

/// Every letter `code`'s layout shows types itself, where `drawn` says what
/// the fonts draw; at least `least` letters are shown.
fn types_itself(code: &str, drawn: &dyn Fn(&str) -> bool, least: usize) {
    let setting = alloc::format!("keyboard.layout.{code}");
    let chosen = crate::layouts::chosen_drawn("en", "", &setting, false, &[], drawn).unwrap();
    let mut typing = Typing::of(
        Board::new(
            look(X4_PRO),
            own("keyboard.size.medium", false),
            plain(),
            chosen,
            0,
            &[],
        )
        .unwrap(),
    );
    let shown = letters(&typing.board);
    assert!(shown.len() >= least, "{code}: {shown:?}");
    let mut typed = String::new();
    for letter in &shown {
        typing.tap(letter);
        typed.push_str(letter);
    }
    assert_eq!(typing.text(), typed, "{code}");
}

#[test]
fn every_letter_of_every_drawable_layout_types_itself() {
    let codes = [
        "de", "fr", "es", "pt", "sv", "da", "cs", "tr", "ru", "uk", "el",
    ];
    for code in codes {
        // A layout the fonts cannot draw falls back to the language's (26
        // letters); where they draw it, its own letters are all there (the
        // shortest, Greek, has 24).
        types_itself(code, &latin_only, 26);
        types_itself(code, &|_| true, 24);
    }
}

#[test]
fn unknown_layouts_fall_back_to_the_language() {
    let typing = Typing::of(board_with(
        X4_PRO,
        plain(),
        "keyboard.layout.xx, , nonsense",
        &[],
    ));
    assert!(find(&typing.board, "q").is_some());
    assert!(find(&typing.board, "w").is_some());
    // One layout: no key to switch.
    assert!(find(&typing.board, "EN").is_none());
    assert_eq!(typing.board.layouts.len(), 1);
}

#[test]
fn a_secret_keeps_the_letter_rows_of_any_text() {
    // The keyboard of a password types as any other.
    let mut typing = Typing::new(X4_PRO, secret());
    typing.tap_all("pw");
    assert_eq!(typing.text(), "pw");
    assert_eq!(typing.board.layer, Layer::Letters);
}
