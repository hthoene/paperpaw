//! The drawing helpers every screen is made of, in the UI font PaperPaw
//! ships: rows, switches, numbers, buttons, headers, pagers, notes, empty
//! screens and the row of hints draw inside the rectangle they are given
//! (long texts shortened, never run into what is next to them), keep to
//! the same margins, and a header's or pager's arrows are hit where they
//! are drawn, on touch screens only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Color, Hint, Rect};
use proptest::prelude::*;

use support::{Device, Harness, drawn_outside, frame};

// ---- Shortening -------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]


}

// ---- Rows ---------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]


}

// ---- Header and pager --------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]


}

// ---- Messages, notes, empty screens, hints ------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]


    /// The row of hints draws each button's hint in its share of the
    /// width, inside the row, the pressed one inverted.
    #[test]
    fn the_row_of_hints_draws_inside_it(
        hints in proptest::collection::vec(
            prop_oneof![
                Just(Hint::Default),
                Just(Hint::Hidden),
                Just(Hint::Icon("arrow-left")),
                "[a-zA-Z ]{0,30}".prop_map(Hint::Text),
            ],
            0..6,
        ),
        pressed in proptest::option::of(0usize..6),
    ) {
        let h = Harness::new(Device::BUTTONS);
        let env = h.env();
        let height = ui::button_hints_height(&env);
        let area = Rect::new(0, 800 - height, 480, height);
        let mut frame = frame(480, 800);
        ui::draw_button_hints(&mut frame.canvas(), &env, area, &hints, pressed);
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
    }
}

/// The ink of hint row `area` in `frame` (what is not white below its
/// line): its columns from left to right.
fn hint_ink(frame: &paperpaw_graphics::Framebuffer, area: Rect) -> Vec<u16> {
    let view = frame.view();
    (area.x..area.right() as u16)
        .filter(|&x| (area.y + 4..area.bottom() as u16).any(|y| view.pixel(x, y) != Color::WHITE))
        .collect()
}

/// Two hints too long for their shares side by side keep half a gap or
/// more between their words.
#[test]
fn long_hints_side_by_side_stay_apart() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let gap = UiStyle::new(&env).gap();
    let height = ui::button_hints_height(&env);
    let area = Rect::new(0, 800 - height, 480, height);
    let long = |c: char| Hint::Text(std::iter::repeat_n(c, 30).collect());
    let hints = [long('W'), long('M'), long('W'), long('M')];
    let mut frame = frame(480, 800);
    ui::draw_button_hints(&mut frame.canvas(), &env, area, &hints, None);
    let ink = hint_ink(&frame, area);
    for boundary in [120, 240, 360] {
        let near: Vec<_> = ink
            .iter()
            .filter(|&&x| x + gap / 2 > boundary && x < boundary + gap / 2)
            .collect();
        assert!(near.is_empty(), "ink at {near:?} near {boundary}");
    }
}

/// A pressed hint whose words reach past its share has its box round all
/// of them: white words never on the white row.
#[test]
fn a_pressed_hint_keeps_its_words_in_its_box() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let height = ui::button_hints_height(&env);
    let area = Rect::new(0, 800 - height, 480, height);
    let hints = [
        Hint::Text("Back".into()),
        Hint::Text("Without a router at all".into()),
        Hint::Hidden,
        Hint::Icon("arrow-left"),
    ];
    let mut plain = frame(480, 800);
    ui::draw_button_hints(&mut plain.canvas(), &env, area, &hints, None);
    // The words of the second hint, where they are drawn.
    let words: Vec<u16> = hint_ink(&plain, area)
        .into_iter()
        .filter(|&x| (125..355).contains(&x))
        .collect();
    let (first, last) = (words[0], words[words.len() - 1]);
    assert!(
        last > 240,
        "the words reach past their share: {first}..{last}"
    );
    let mut pressed = frame(480, 800);
    ui::draw_button_hints(&mut pressed.canvas(), &env, area, &hints, Some(1));
    let view = pressed.view();
    let middle = area.y + area.height / 2 + 1;
    for x in [first - 1, last + 1] {
        assert_eq!(view.pixel(x, middle), Color::BLACK, "box at {x}");
    }
}

/// The text the kernel makes of an icon's character (for an icon a native
/// plugin named) draws as the icon named does, pressed or not; other
/// texts stay words.
#[test]
fn an_icons_text_draws_as_the_icon_named() {
    use paperpaw_sdk::icons;
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let height = ui::button_hints_height(&env);
    let area = Rect::new(0, 800 - height, 480, height);
    let text = icons::char("arrow-left").and_then(icons::as_text).unwrap();
    let row = |shown: Hint, pressed| {
        let hints = [Hint::Text("Back".into()), shown, Hint::Hidden];
        let mut drawn = frame(480, 800);
        ui::draw_button_hints(&mut drawn.canvas(), &env, area, &hints, pressed);
        let view = drawn.view();
        (area.y..area.bottom() as u16)
            .flat_map(|y| (160..320).map(move |x| (x, y)))
            .filter(|&(x, y)| view.pixel(x, y) != Color::WHITE)
            .collect::<Vec<_>>()
    };
    for pressed in [None, Some(1)] {
        let named = row(Hint::Icon("arrow-left"), pressed);
        assert!(named.len() > 20, "the icon is drawn: {}", named.len());
        assert_eq!(row(Hint::Text(text.clone()), pressed), named);
        // Not an icon's text: more after the character, or no mark before.
        assert_ne!(row(Hint::Text(format!("{text}a")), pressed), named);
        assert_ne!(row(Hint::Text(text[3..].to_string()), pressed), named);
    }
}
