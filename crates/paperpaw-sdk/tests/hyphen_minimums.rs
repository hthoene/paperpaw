//! The letters kept before and after a hyphen are the reader's settings:
//! the page's patterns carry them (0: the language's own), they are in the
//! page's key where they matter, so books lay out again when they change,
//! and a page that keeps the language's own keeps the key it always had.

// The reading functions come with the `unstable` feature.
#![cfg(feature = "unstable")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::reading::{PageType, StatusRoom};
use paperpaw_sdk::{HyphenationSpec, Id, ui};

use support::{Device, Harness};

/// A reader's settings: hyphenation on or off, and the minimums (`None`:
/// the settings are not even there, as before they existed).
fn harness(hyphenate: bool, minimums: Option<(i64, i64)>) -> Harness {
    let mut h = Harness::new(Device::BUTTONS);
    h.define_bool(settings::HYPHENATE.as_str(), hyphenate);
    if let Some((left, right)) = minimums {
        h.define_int(settings::HYPHEN_MIN_LEFT.as_str(), left, -10, 99);
        h.define_int(settings::HYPHEN_MIN_RIGHT.as_str(), right, -10, 99);
    }
    // Patterns made for 2 letters before and 2 after.
    let table = Box::leak(paperpaw_sdk::hyphen::compile("1b", "", 2, 2).into_boxed_slice());
    h.hyphenation
        .add(HyphenationSpec::new(Id::new_static("test.en"), "en", table))
        .unwrap();
    h
}

fn page(h: &Harness, language: &str) -> PageType {
    let env = h.env();
    let font = ui::UiStyle::new(&env).font(&env).unwrap();
    PageType::new(&env, &font, language, false, StatusRoom::NONE)
}

fn page_of(h: &Harness) -> PageType {
    page(h, "en")
}

#[test]
fn values_beyond_the_settings_range_are_held_to_it() {
    let h = harness(true, Some((99, -4)));
    assert_eq!(page_of(&h).hyphen_minimums, (5, 0));
}

#[test]
fn a_change_gives_the_book_another_key() {
    let key = |hyphenate, minimums| page_of(&harness(hyphenate, minimums)).key;
    let own = key(true, Some((0, 0)));
    let others = [(1, 0), (0, 1), (1, 1), (2, 3), (3, 2), (5, 5)];
    let keys: Vec<String> = others.iter().map(|&m| key(true, Some(m))).collect();
    for (i, k) in keys.iter().enumerate() {
        assert_ne!(*k, own, "{:?}", others[i]);
        // Each pair of values is a layout of its own.
        for (j, other) in keys.iter().enumerate() {
            assert!(i == j || k != other, "{:?} {:?}", others[i], others[j]);
        }
    }
    // The same values, the same key.
    assert_eq!(key(true, Some((2, 3))), key(true, Some((2, 3))));
}

#[test]
fn the_languages_own_keeps_the_key_it_always_had() {
    // Before the settings existed there was no such part in the key.
    assert_eq!(
        page_of(&harness(true, Some((0, 0)))).key,
        page_of(&harness(true, None)).key
    );
}
