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

fn breaks(page: &PageType, h: &Harness, word: &str) -> Vec<usize> {
    let patterns = page.patterns(&h.env()).unwrap();
    let mut found = Vec::new();
    patterns.hyphenate(word, &mut |at| found.push(at));
    found
}

#[test]
fn the_page_carries_the_minimums_and_0_is_the_languages_own() {
    let h = harness(true, Some((0, 0)));
    let page = page(&h, "en");
    assert_eq!(page.hyphen_minimums, (0, 0));
    assert_eq!(breaks(&page, &h, "abababa"), [3, 5]);
    // "abab" can take no hyphen with 2 and 2.
    assert!(breaks(&page, &h, "abab").is_empty());

    let h = harness(true, Some((1, 1)));
    let page = page_of(&h);
    assert_eq!(breaks(&page, &h, "abababa"), [1, 3, 5]);
    assert_eq!(breaks(&page, &h, "abab"), [1, 3]);

    // One given, the other the language's own.
    let h = harness(true, Some((0, 3)));
    assert_eq!(breaks(&page_of(&h), &h, "abababa"), [3]);
    let h = harness(true, Some((4, 0)));
    assert_eq!(breaks(&page_of(&h), &h, "abababa"), [5]);
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

#[test]
fn the_minimums_do_not_matter_where_nothing_is_hyphenated() {
    // Switched off, or no patterns for the book's language.
    let off = |minimums| page_of(&harness(false, minimums));
    assert_eq!(off(Some((3, 3))).key, off(Some((0, 0))).key);
    assert!(
        off(Some((3, 3)))
            .patterns(&harness(false, None).env())
            .is_none()
    );
    let h = harness(true, Some((3, 3)));
    let no_patterns = page(&h, "xx");
    assert!(no_patterns.hyphenation.is_none());
    assert!(no_patterns.patterns(&h.env()).is_none());
    assert_eq!(
        no_patterns.key,
        page(&harness(true, Some((0, 0))), "xx").key
    );
}
