//! The plugin's texts: both catalogs say the same, and every text the code
//! asks for is there.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use alloc::vec::Vec;
use paperpaw_i18n::{Catalog, check};

const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn translations_are_complete() {
    let en = Catalog::parse(EN).unwrap();
    let de = Catalog::parse(DE).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

/// The keys the sources name as `"transfer.…"` (the plugin's own texts).
fn keys_named(source: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for part in source.split("\"transfer.").skip(1) {
        if let Some((key, _)) = part.split_once('"') {
            found.push(key);
        }
    }
    found
}

/// A text the code asks for by name is in the catalog: a typo would show the
/// key on the screen.
#[test]
fn every_text_the_code_asks_for_is_there() {
    let en = Catalog::parse(EN).unwrap();
    let sources = [
        include_str!("lib.rs"),
        include_str!("native_plugin.rs"),
        include_str!("native_plugin/own.rs"),
        include_str!("native_plugin/phone.rs"),
        include_str!("native_phone.rs"),
        include_str!("native_install.rs"),
    ];
    let mut seen = 0;
    for source in sources {
        for key in keys_named(source) {
            // Ids that are not texts: the action, the screen, the timer's key.
            if ["open", "screen", "own-idle"].contains(&key) {
                continue;
            }
            let full = alloc::format!("transfer.{key}");
            assert!(en.get(&full).is_some(), "{full}");
            seen += 1;
        }
    }
    assert!(seen > 30, "{seen}");
}
