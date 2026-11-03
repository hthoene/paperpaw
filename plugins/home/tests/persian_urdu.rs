//! The Persian and Urdu interface packs as shipped (`tools/plugin-files/languages/fa.ini`
//! and `ur.ini`, #33): the home screen shows their texts, writes numbers in the
//! extended Arabic-Indic digits both languages use, runs from right to left, and shows
//! nothing but Arabic-script letters, Latin names and digits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{Id, Params};

const DRAWER: &str = "home.drawer";

const FA: &str = include_str!("../../../tools/plugin-files/languages/fa.ini");
const UR: &str = include_str!("../../../tools/plugin-files/languages/ur.ini");

/// The shipped catalog of `language` and the digits it writes.
fn catalog(language: &str) -> &'static str {
    match language {
        "fa" => FA,
        "ur" => UR,
        other => panic!("no catalog for {other}"),
    }
}

/// What the catalog says for `key`.
fn message(language: &str, key: &str) -> String {
    let prefix = format!("{key} = ");
    catalog(language)
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("{language} has no {key}"))
        .to_string()
}

/// A card with the pack installed as the store does (a data plugin whose
/// catalog is compiled, `i18n/<language>.ppcat`, ADR 0033) and the language
/// chosen.
fn card(language: &str) -> Shared {
    let plugin = format!(
        "[plugin]\nid = language-{language}\nname = {language}\nversion = 1.0.0\nformats = 4\n"
    );
    let compiled = paperpaw_i18n::Catalog::compile(catalog(language)).unwrap();
    Shared::memory(
        &format!("[core]\nlanguage = {language}\n[help]\nshow = false\n"),
        &[
            (
                &format!("plugins/language-{language}/plugin.ini"),
                plugin.as_bytes(),
            ),
            (
                &format!("plugins/language-{language}/i18n/{language}.ppcat"),
                &compiled,
            ),
        ],
    )
}

fn is_extended_arabic_indic(c: char) -> bool {
    ('\u{06F0}'..='\u{06F9}').contains(&c)
}

#[test]
fn the_home_screen_speaks_the_language_chosen() {
    for language in ["fa", "ur"] {
        for device in DEVICES {
            let mut app = App::new(device, card(language));
            let title = message(language, "home.start.title");
            assert!(
                app.shows(&title),
                "{language}: {title:?} not among {:?}",
                app.labels()
            );
            assert!(!app.shows("Start reading"), "{language}: still English");
        }
    }
}

#[test]
fn numbers_the_sdk_writes_come_in_the_extended_arabic_indic_digits() {
    for language in ["fa", "ur"] {
        let mut app = App::new(X4, card(language));
        app.open_screen(Id::new_static("battery.screen"), Params::new());
        let shown = app.labels().join(" | ");
        assert!(
            shown.chars().any(is_extended_arabic_indic),
            "{language}: no digits of the language in {shown}"
        );
    }
    // English keeps its own.
    let mut app = App::new(X4, Shared::memory("[help]\nshow = false\n", &[]));
    app.open_screen(Id::new_static("battery.screen"), Params::new());
    let shown = app.labels().join(" | ");
    assert!(shown.chars().any(|c| c.is_ascii_digit()), "{shown}");
}

#[test]
fn every_label_is_made_of_arabic_script_latin_names_digits_and_punctuation() {
    for language in ["fa", "ur"] {
        for device in DEVICES {
            let mut app = App::new(device, card(language));
            let mut labels = app.labels();
            app.open_screen(Id::new_static(DRAWER), Params::new());
            labels.extend(app.labels());
            for label in &labels {
                for c in label.chars() {
                    let arabic = ('\u{0600}'..='\u{06FF}').contains(&c)
                        || ('\u{0750}'..='\u{077F}').contains(&c)
                        || ('\u{FB50}'..='\u{FDFF}').contains(&c)
                        || ('\u{FE70}'..='\u{FEFF}').contains(&c);
                    let other = c.is_ascii()
                        || matches!(c, '\u{200C}' | '\u{200D}' | '\u{200E}' | '\u{200F}')
                        || c.is_whitespace()
                        || matches!(c, '…' | '·' | '›' | '‹' | '«' | '»' | '–' | '—');
                    assert!(arabic || other, "{language}: {c:?} in {label:?}");
                }
                // The Arabic yeh and kaf are Arabic's, not Persian's or Urdu's.
                assert!(
                    !label.contains('\u{064A}') && !label.contains('\u{0643}'),
                    "{language}: Arabic yeh or kaf in {label:?}"
                );
            }
        }
    }
}

#[test]
fn the_drawer_starts_at_the_top_right_in_both_languages() {
    for language in ["fa", "ur"] {
        let mut app = App::new(X4, card(language));
        app.open_screen(Id::new_static(DRAWER), Params::new());
        assert_eq!(app.screen(), DRAWER);
        let tools: Vec<_> = app
            .parts()
            .into_iter()
            .filter(|p| p.role == Role::Button)
            .map(|p| p.area)
            .collect();
        assert!(tools.len() > 4, "{language}: {tools:?}");
        assert!(tools[0].x > tools[1].x, "{language}: {tools:?}");
        assert_eq!(tools[0].y, tools[1].y, "{language}: {tools:?}");
    }
}
