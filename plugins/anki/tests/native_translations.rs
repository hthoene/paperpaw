#![allow(clippy::unwrap_used, reason = "test assertions")]
//! The two catalogs are complete against each other, and every text the plugin
//! asks for exists in both (plural ones by their `.one` and `.other`).
use paperpaw_i18n::{Catalog, check};

const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

fn has(catalog: &str, key: &str) -> bool {
    [
        format!("{key} = "),
        format!("{key}.one = "),
        format!("{key}.other = "),
    ]
    .iter()
    .any(|line| catalog.lines().any(|l| l.starts_with(line.as_str())))
}

#[test]
fn the_catalogs_agree() {
    let en = Catalog::parse(EN).unwrap();
    let de = Catalog::parse(DE).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn every_text_the_code_asks_for_is_in_both_languages() {
    let mut asked = 0;
    for source in [
        include_str!("../src/native_plugin.rs"),
        include_str!("../src/native_study.rs"),
        include_str!("../src/native_study_screen.rs"),
        include_str!("../src/native_sync.rs"),
        include_str!("../declarations.rs"),
    ] {
        for part in source.split('"').skip(1).step_by(2) {
            // A key of this plugin: `anki.` and a name, not an id of a screen
            // or an action (those are the declared ones, which have titles).
            let key = part.starts_with("anki.")
                && !part.contains(' ')
                && !matches!(part, "anki.home" | "anki.decks" | "anki.open");
            if key {
                asked += 1;
                for (name, catalog) in [("en", EN), ("de", DE)] {
                    assert!(has(catalog, part), "{part} missing in {name}");
                }
            }
        }
    }
    assert!(asked >= 30, "{asked}");
}

#[test]
fn the_how_it_works_texts_explain_the_add_on_settings() {
    for text in [include_str!("../how/en.txt"), include_str!("../how/de.txt")] {
        for word in ["webBindAddress", "0.0.0.0", "apiKey", "8765", "webBindPort"] {
            assert!(text.contains(word), "{word}");
        }
    }
}
