//! The English and German texts agree, and every text the plugin asks for
//! exists in both.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
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

#[test]
fn every_key_the_plugin_asks_for_is_in_both_languages() {
    let sources = [
        include_str!("../src/native_plugin.rs"),
        include_str!("../declarations.rs"),
    ];
    let mut asked = 0;
    for source in sources {
        for part in source.split('"').skip(1).step_by(2) {
            if part.starts_with("plugin-manager.")
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'.')
                && !part.ends_with('.')
                && part != "plugin-manager.list"
                && part != "plugin-manager.screens.open"
                && part != "plugin-manager.more"
            {
                asked += 1;
                for (name, catalog) in [("en", EN), ("de", DE)] {
                    assert!(
                        catalog
                            .lines()
                            .any(|l| l.starts_with(&format!("{part} = "))),
                        "{part} missing in {name}"
                    );
                }
            }
        }
    }
    assert!(asked >= 20, "{asked}");
}
