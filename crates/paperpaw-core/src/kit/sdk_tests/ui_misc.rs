//! The smaller UI pieces: questions for text and folders ([`ask`]),
//! icons by name, what a screen tells about itself ([`semantics`]) as
//! the drawing helpers describe it, menus offered by what the device can
//! do, the drawn logo, and the shared look of widgets (frames, captions,
//! wrapped text) and their placements.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

extern crate std;
#[allow(
    unused_imports,
    reason = "the names of std the tests had as SDK integration tests, not all used"
)]
use std::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use super::support;

use std::borrow::Cow;

use crate::kit::menus::MenuEntry;
use crate::kit::menus::MenuNeed;
use crate::kit::menus::MenuSection;
use crate::kit::menus::Menus;
use crate::kit::ui::UiStyle;
use crate::kit::widgets::{self};
use paperpaw_sdk::builtin::params;
use paperpaw_sdk::semantics::{self};
use paperpaw_sdk::{Capabilities, Id, Params, Rect, icons};
use proptest::prelude::*;

use support::{Device, Harness, drawn_in, drawn_outside, frame};

// ---- Questions ----------------------------------------------------------

// ---- Icons ----------------------------------------------------------------

#[test]
fn every_icon_is_found_by_its_name() {
    let names: Vec<&str> = icons::names().collect();
    assert!(names.windows(2).all(|w| w[0] < w[1]), "sorted, each once");
    for name in &names {
        assert!(!name.is_empty());
        let c = icons::char(name).unwrap_or_else(|| panic!("{name}"));
        // Past the letters, where icon fonts keep their drawings.
        assert!(c >= '\u{e000}', "{name}: {c:?}");
    }
}

/// The names and characters of Tabler Icons 3.35.0 as the table had them
/// before it was packed (`name=U+code` a line, FNV-1a): the packing
/// changes none.
#[test]
fn the_packed_table_holds_the_same_names_and_characters() {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut count = 0;
    for name in icons::names() {
        let c = icons::char(name).unwrap_or_else(|| panic!("{name}"));
        for byte in format!("{name}=U+{:04X}\n", u32::from(c)).bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        count += 1;
    }
    assert_eq!(count, 5934);
    assert_eq!(hash, 0x8a8d_e240_0cd5_330a);
}

/// A name's hash as the table keeps it in place of the name (FNV-1a, 32
/// bits; `tools/icons/subset.py`).
fn name_hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash: u32, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Whether `name` is taken for an icon: its hash is an icon name's. The
/// table holds no names (#258), so that is all it can tell.
fn hashes_as_an_icon(name: &str) -> bool {
    static HASHES: std::sync::OnceLock<std::collections::BTreeSet<u32>> =
        std::sync::OnceLock::new();
    HASHES
        .get_or_init(|| icons::names().map(name_hash).collect())
        .contains(&name_hash(name))
}

/// No two icons' names hash alike, and names that are no icon's (near
/// ones, none at all, what an icon means) are not found.
#[test]
fn names_are_told_apart_by_their_hashes() {
    let hashes: std::collections::BTreeSet<u32> = icons::names().map(name_hash).collect();
    assert_eq!(hashes.len(), 5934);
    for name in [
        "",
        "no-such-icon",
        "book-",
        "books2",
        "Books",
        "arrow",
        icons::BACK,
    ] {
        assert_eq!(icons::char(name), None, "{name}");
    }
    assert_eq!(icons::char("U+F8000"), Some('\u{f8000}'));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Any name at all is looked up without a panic: an icon's own, or
    /// none (but for one whose hash is an icon name's).
    #[test]
    fn any_name_is_looked_up_safely(name in "\\PC{0,30}") {
        let found = icons::char(&name);
        prop_assert_eq!(found.is_some(), hashes_as_an_icon(&name));
    }

    /// Names next to real ones (cut short or longer, as `a-b` is of
    /// `a-b-2`) are found exactly when their hash is an icon name's, and
    /// an icon's own name gives its own character.
    #[test]
    fn names_next_to_real_ones_are_told_apart(
        index in 0..5934usize,
        cut in 0..40usize,
        tail in "[a-z0-9-]{0,4}",
    ) {
        let real = icons::names().nth(index).unwrap();
        let name = format!("{}{tail}", &real[..cut.min(real.len())]);
        let found = icons::char(&name);
        prop_assert_eq!(found.is_some(), hashes_as_an_icon(&name));
        if icons::names().any(|n| n == name) {
            prop_assert!(found.is_some(), "{}", name);
        }
    }
}

// ---- What a screen tells ------------------------------------------------

#[test]
fn nothing_told_tells_nothing() {
    assert!(semantics::parts(Vec::new()).is_empty());
    assert!(semantics::focused(&[]).is_none());
    assert!(semantics::find(&[], "").is_none());
}

// ---- Menus ----------------------------------------------------------------

const NEEDS: [MenuNeed; 9] = [
    MenuNeed::Speech,
    MenuNeed::Sleep,
    MenuNeed::Rotation,
    MenuNeed::NoTouch,
    MenuNeed::ButtonRow,
    MenuNeed::Colors,
    MenuNeed::Wifi,
    MenuNeed::Firmware,
    MenuNeed::BlackAndWhite,
];

fn caps_of(bits: u16) -> Capabilities {
    let mut caps = Capabilities::default();
    let flags: [&mut bool; 9] = [
        &mut caps.speech,
        &mut caps.sleep,
        &mut caps.rotation,
        &mut caps.touch,
        &mut caps.button_row,
        &mut caps.colors,
        &mut caps.wifi,
        &mut caps.firmware,
        &mut caps.black_and_white,
    ];
    for (n, flag) in flags.into_iter().enumerate() {
        *flag = bits & (1 << n) != 0;
    }
    caps
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// An entry that needs something is offered where the device has it
    /// (one for devices without touch: where there is none, or where the
    /// user listed it), in the plugins' menus and the user's alike; an
    /// entry without a need everywhere.
    #[test]
    fn entries_are_offered_where_the_device_can_do_what_they_need(bits in 0u16..512) {
        let caps = caps_of(bits);
        let mut menus = Menus::new();
        menus.set_capabilities(caps);
        let menu = Id::new_static("t.menu");
        let mut ids = Vec::new();
        for (n, need) in NEEDS.iter().enumerate() {
            let id = Id::parse(&format!("t.entry{n}")).unwrap();
            menus.add(MenuEntry::new(menu.clone(), id.clone()).order(n as i16));
            menus.set_title(id.clone(), Cow::Borrowed("t.title"));
            menus.set_need(id.clone(), *need);
            ids.push(id);
        }
        let plain = Id::new_static("t.plain");
        menus.add(MenuEntry::new(menu.clone(), plain.clone()).order(99));
        menus.set_title(plain.clone(), Cow::Borrowed("t.title"));
        let has = [
            caps.speech, caps.sleep, caps.rotation, !caps.touch, caps.button_row,
            caps.colors, caps.wifi, caps.firmware, caps.black_and_white,
        ];
        let shown: Vec<&Id> = menus.ids("t.menu");
        let mut expected: Vec<&Id> = ids.iter().zip(has).filter(|(_, h)| *h).map(|(id, _)| id).collect();
        expected.push(&plain);
        prop_assert_eq!(&shown, &expected);
        for (id, has) in ids.iter().zip(has) {
            prop_assert_eq!(menus.available(id.as_str()), has);
        }
        // Listed by the user: the same, but for devices without touch,
        // which the user may list anywhere.
        let mut all = ids.clone();
        all.push(plain.clone());
        menus.customize(menu.clone(), all);
        let mut expected: Vec<&Id> = ids
            .iter()
            .zip(has)
            .enumerate()
            .filter(|(n, (_, h))| *h || NEEDS[*n] == MenuNeed::NoTouch)
            .map(|(_, (id, _))| id)
            .collect();
        expected.push(&plain);
        let shown = menus.ids("t.menu");
        prop_assert_eq!(shown, expected);
    }
}

#[test]
fn entries_come_in_their_order_headings_among_them() {
    let mut menus = Menus::new();
    let menu = Id::new_static("t.menu");
    for (item, order) in [("t.c", 30), ("t.a", 10), ("t.b", 10), ("t.d", -5)] {
        let id = Id::parse(item).unwrap();
        menus.add(MenuEntry::new(menu.clone(), id.clone()).order(order));
        menus.set_title(id, Cow::Borrowed(item));
    }
    menus.add_section(
        MenuSection::new(Id::new_static("t.heading"), "t.heading.title", menu.clone()).order(20),
    );
    menus.set_icon(Id::new_static("t.a"), Cow::Borrowed("books"));
    let (actions, settings) = (
        crate::kit::action::Actions::new(),
        paperpaw_sdk::Settings::new(),
    );
    let items = crate::kit::menus::MenuView::new(&menus, &actions, &settings).items("t.menu");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    // Equal orders keep the order they were added in.
    assert_eq!(ids, ["t.d", "t.a", "t.b", "t.heading", "t.c"]);
    assert!(items[3].section && !items[0].section);
    assert_eq!(items[1].icon, Some("books"));
    assert_eq!(menus.order_of("t.menu", "t.heading"), Some(20));
    assert!(menus.menu_ids().contains(&&menu));
    // A user's menu naming what does not exist: left out, no panic.
    menus.customize(
        menu.clone(),
        vec![Id::new_static("t.gone"), Id::new_static("t.c")],
    );
    let ids: Vec<&str> = crate::kit::menus::MenuView::new(&menus, &actions, &settings)
        .items("t.menu")
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(ids, ["t.c"]);
    assert!(menus.is_customized("t.menu"));
    assert_eq!(menus.default_ids("t.menu").len(), 5);
    menus.reset("t.menu");
    assert!(!menus.is_customized("t.menu"));
    // Hidden where a menu is opened so, the list written any way.
    let p = Params::new().with(params::HIDE, " t.a ,t.b,, ");
    assert!(Menus::hidden(&p, "t.a") && Menus::hidden(&p, "t.b"));
    assert!(!Menus::hidden(&p, "t.c"));
}

// ---- The logo -------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

}

// ---- The look of widgets ----------------------------------------------------

fn look_harness(frame_value: &str, align: &str) -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    h.define_text("t.clock.frame", frame_value);
    h.define_text("t.clock.align", align);
    h
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]


}

/// Found by the property above: a caption of a combining mark alone (a
/// diaeresis below) reached back past the area's left edge, onto a
/// letter that is not there. It sits on the start of its line instead.
#[test]
fn a_caption_of_a_lone_mark_keeps_to_its_area() {
    let h = look_harness("core.frame.none", "");
    let env = h.env();
    let area = Rect::new(3, 0, 100, 99);
    let mut frame = frame(480, 800);
    let look = widgets::draw_frame(&mut frame.canvas(), &env, "t.clock", area);
    let mut frame = support::frame(480, 800);
    widgets::draw_caption(&mut frame.canvas(), &env, "t.clock", &look, "\u{324}");
    assert!(drawn_in(&frame, look.area), "the mark is drawn");
    assert_eq!(drawn_outside(&mut frame, look.area), None);
}

/// The lines of a heading on more lines are closer than those of text,
/// and still the highest letters of a line (capitals with accents) stay
/// clear of the lowest of the line above.
#[test]
fn heading_lines_are_closer_than_text_and_never_touch() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let font = UiStyle::new(&env).font(&env).unwrap();
    for size in [12.0, 16.0, 20.0, 26.0, 34.0, 48.0] {
        let apart = font.heading_line_height(size);
        assert!(apart < font.line_metrics(size).line_height, "{size}");
        assert_eq!(widgets::heading_line_height(&env, size), apart as u16);
        let (top, _) = font.ink(size, "ÅÄÉÖÜ").unwrap();
        let (_, bottom) = font.ink(size, "gjpqyß").unwrap();
        assert!(
            ((bottom - top) as f32) < apart,
            "{size}: {top} {bottom} {apart}"
        );
    }
}
