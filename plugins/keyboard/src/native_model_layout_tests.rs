//! Native replacements of the interface-6 tests of `typing.rs` about text that
//! comes from elsewhere (a phone, the clipboard, a hardware keyboard), about
//! layouts other plugins add, Korean jamo, and a layout far too big, on a
//! [`Board`]. The dialog's part (the nested question, the clipboard, event
//! 109) is in `tests/acceptance_pending.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::layouts::{Added, chosen_drawn};
use crate::native_layouts::from_fields;
use crate::native_model::{Asked, Board, FieldButton, MAX_CHARS, Move, Pressed};
use crate::native_model_tests::{
    BOOX, Typing, X4, X4_PRO, board, board_with, find, look, own, plain,
};
use alloc::string::{String, ToString};
use proptest::prelude::*;

fn number() -> Asked {
    Asked {
        number: true,
        ..Asked::default()
    }
}

fn on_phone(asked: Asked, before: &str, text: &str) -> String {
    let mut typing = Typing::new(X4_PRO, asked);
    typing.board.text = String::from(before);
    typing.board.replace_pasted(text).unwrap();
    typing.board.text.clone()
}

#[test]
fn text_from_a_phone_replaces_the_text_as_one_line() {
    assert_eq!(
        on_phone(plain(), "before", "  first\nsecond\tthird\u{7}  "),
        "first second third"
    );
}

#[test]
fn text_from_a_phone_is_cut_to_the_longest_allowed() {
    let cut = on_phone(plain(), "", &"ä".repeat(MAX_CHARS * 2));
    assert_eq!(cut, "ä".repeat(MAX_CHARS));
}

#[test]
fn text_from_a_phone_keeps_only_digits_for_numbers() {
    assert_eq!(on_phone(number(), "", "PIN: 12-34 ٣"), "1234");
}

/// Text typed from outside (keys of a keyboard, a remote control) goes in as
/// if typed on the keys, umlauts and spaces kept, and what is no text left
/// out.
#[test]
fn text_typed_from_outside_goes_in_as_typed() {
    let mut typing = Typing::new(X4, plain());
    typing.board.typed("Grüße, ").unwrap();
    typing.board.typed("Zoë €5\n").unwrap();
    assert_eq!(typing.text(), "Grüße, Zoë €5");
    typing.tap("Space");
    assert_eq!(typing.text(), "Grüße, Zoë €5 ");
    // A line break is text where lines are typed.
    let mut lines = Typing::new(X4_PRO, crate::native_model_tests::lines());
    lines.board.typed("a\nb\u{7}").unwrap();
    assert_eq!(lines.text(), "a\nb");
    let mut touch = Typing::new(X4_PRO, plain());
    touch.board.typed("ab").unwrap();
    assert_eq!(touch.text(), "ab");
}

/// Jamo from outside join to syllables as they come; a layout of letters the
/// fonts lack is not offered; text from a phone is taken as it is.
#[test]
fn korean_jamo_join_to_syllables() {
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.typed("ㅎㅏㄴㄱㅡㄹ").unwrap();
    assert_eq!(typing.text(), "한글");

    // A letter of Latin text between: nothing joins across it.
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.typed("ㄱaㅏ").unwrap();
    assert_eq!(typing.text(), "ㄱaㅏ");

    // Text from a phone is not joined to what is there.
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.replace_pasted("ㅎㅏ").unwrap();
    assert_eq!(typing.text(), "ㅎㅏ");
}

fn korean() -> Added {
    from_fields(
        "keyboard.layout.ko",
        "KO",
        "ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ ㅁㄴㅇㄹㅎㅗㅓㅏㅣ ㅋㅌㅊㅍㅠㅜㅡ",
        "",
    )
    .unwrap()
    .unwrap()
}

#[test]
fn the_korean_layout_types_syllables_and_takes_back_a_jamo_at_a_time() {
    let drawn = chosen_drawn("en", "", "keyboard.layout.ko", false, &[korean()], &|_| {
        true
    });
    let mut typing = Typing::of(
        Board::new(
            look(X4_PRO),
            own("keyboard.size.medium", false),
            plain(),
            drawn.unwrap(),
            0,
            &[],
        )
        .unwrap(),
    );
    assert!(typing.board.composing());
    typing.tap_all("ㅎㅏㄴ");
    assert_eq!(typing.text(), "한");
    typing.tap("Delete");
    assert_eq!(typing.text(), "하");
    typing.tap("Delete");
    assert_eq!(typing.text(), "ㅎ");
    // Not where the fonts lack Hangul: the language's layout stands in.
    let no_hangul = |text: &str| {
        text.chars()
            .all(|c| !('\u{3130}'..='\u{318F}').contains(&c))
    };
    let chosen = chosen_drawn(
        "en",
        "",
        "keyboard.layout.ko",
        false,
        &[korean()],
        &no_hangul,
    );
    let typing = Typing::of(
        Board::new(
            look(X4_PRO),
            own("keyboard.size.medium", false),
            plain(),
            chosen.unwrap(),
            0,
            &[],
        )
        .unwrap(),
    );
    assert!(find(&typing.board, "ㅎ").is_none());
    assert!(find(&typing.board, "q").is_some());
}

fn dutch() -> Added {
    from_fields("dutch.layout", "NL", "qwertyuiop asdfghjkl zxcvbnm", "i=ïí")
        .unwrap()
        .unwrap()
}

#[test]
fn a_layout_another_plugin_adds_is_typed_with() {
    let chosen = chosen_drawn(
        "en",
        "",
        "keyboard.layout.en, dutch.layout",
        false,
        &[dutch()],
        &|_| true,
    )
    .unwrap();
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
    // English first, then the added one with its code.
    typing.tap("EN");
    assert!(find(&typing.board, "NL").is_some());
    // Its own variants.
    assert!(typing.hold("i"));
    assert_eq!(
        typing.board.popup.as_ref().unwrap().chars[..3],
        ['I', 'ï', 'í']
    );
    typing.choose('ï');
    assert_eq!(typing.text(), "ï");
}

/// A layout far too big stays on the screen: its keys within it, a long press
/// on a letter of many variants too.
#[test]
fn a_layout_far_too_big_stays_on_the_screen() {
    let rows = ["abcdefghijklmnopqrstuvwxyz"; 30].join(" ");
    let variants = alloc::format!("a={}", "é".repeat(500));
    let huge = from_fields("huge.layout", "HUGE", &rows, &variants)
        .unwrap()
        .unwrap();
    assert_eq!(huge.letters.split(' ').count(), crate::layouts::MAX_ROWS);
    for screen in [X4_PRO, X4, BOOX] {
        let chosen = chosen_drawn(
            "en",
            "",
            "huge.layout",
            false,
            core::slice::from_ref(&huge),
            &|_| true,
        );
        let mut typing = Typing::of(
            Board::new(
                look(screen),
                own("keyboard.size.medium", false),
                plain(),
                chosen.unwrap(),
                0,
                &[],
            )
            .unwrap(),
        );
        let b = &typing.board;
        assert!(
            b.region.y + b.region.height <= screen.height,
            "{}",
            screen.name
        );
        for (row, keys) in b.rows.iter().enumerate() {
            for index in 0..keys.len() {
                let a = b.key_area(row, index);
                assert!(
                    a.x + a.width <= screen.width,
                    "{} {row}/{index}",
                    screen.name
                );
                assert!(
                    a.y + a.height <= screen.height,
                    "{} {row}/{index}",
                    screen.name
                );
            }
        }
        // A long press on a letter of many variants: no overflow.
        assert!(typing.hold("a"));
        let popup = typing.board.popup.as_ref().unwrap();
        assert!(
            popup.area.x + popup.area.width <= screen.width,
            "{}",
            screen.name
        );
        assert!(popup.chars.len() <= 1 + 2 * crate::layouts::MAX_VARIANTS + 1);
    }
}

#[test]
fn paste_inserts_the_clipboard_as_one_line() {
    let mut typing = Typing::new(X4_PRO, plain());
    typing.board.text = String::from("a ");
    typing.board.pasted("  pasted\nline\tend ").unwrap();
    assert_eq!(typing.text(), "a pasted line end");
}

#[test]
fn paste_by_buttons_from_above_the_first_row() {
    let paste = [(FieldButton::Paste, 40.0)];
    let mut typing = Typing::of(board_with(X4, plain(), "keyboard.layout.auto", &paste));
    typing.board.move_focus(Move::Up);
    assert_eq!(typing.focused().as_deref(), Some("Paste"));
    // Select on it pastes.
    assert_eq!(typing.board.buttons[0].0, FieldButton::Paste);
    typing.board.pasted("  pasted\nline\tend ").unwrap();
    // Down again to the key below it, at the right of the first row.
    typing.board.move_focus(Move::Down);
    assert_eq!(typing.focused().as_deref(), Some("o"));
    assert_eq!(typing.text(), "pasted line end");
}

#[test]
fn paste_keeps_only_digits_for_numbers() {
    let mut typing = Typing::new(X4_PRO, number());
    assert_eq!(
        typing.board.pasted("  pasted\nline\tend ").unwrap(),
        Pressed::Changed
    );
    assert_eq!(typing.text(), "");
}

/// A paste in the field of a board without a clipboard button is no key of
/// the keys: the keys are the same with or without the field's buttons.
#[test]
fn the_field_buttons_do_not_change_the_keys() {
    let none = board(X4_PRO, plain());
    let some = board_with(
        X4_PRO,
        plain(),
        "keyboard.layout.auto",
        &[(FieldButton::Phone, 40.0), (FieldButton::Paste, 40.0)],
    );
    assert_eq!(none.rows, some.rows);
    assert_eq!(some.buttons.len(), 2);
    // Phone and Paste lie side by side at the field's end, inside it.
    let (a, b) = (some.buttons[0].1, some.buttons[1].1);
    assert!(a.x + a.width <= b.x && b.x + b.width <= some.field.width);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Whatever arrives from a phone, the answer is one line, trimmed and no
    /// longer than allowed.
    #[test]
    fn text_from_a_phone_is_always_one_short_line(text in any::<String>()) {
        let answer = on_phone(plain(), "", &text);
        prop_assert!(answer.chars().count() <= MAX_CHARS);
        prop_assert!(!answer.chars().any(char::is_control), "{:?}", answer);
        // A line break of two characters (CR LF) is one space.
        let expected: String = text
            .replace("\r\n", "\n")
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .trim()
            .chars()
            .take(MAX_CHARS)
            .collect();
        prop_assert_eq!(answer, expected);
    }

    /// Any sequence of English letters typed by touch comes back as typed.
    #[test]
    fn letters_typed_by_touch_come_back(text in "[a-z ]{0,24}") {
        let mut typing = Typing::new(X4_PRO, plain());
        for c in text.chars() {
            typing.tap(&if c == ' ' { "Space".to_string() } else { c.to_string() });
        }
        prop_assert_eq!(typing.text(), text.as_str());
    }
}
