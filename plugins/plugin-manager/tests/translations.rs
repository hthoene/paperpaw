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

/// A copied code plugin runs: its row says "not signed" and fits the X4 row
/// (it once read "Not installed: ...", which sounds like it does not run).
#[test]
fn the_unsigned_note_says_it_runs_and_is_short() {
    for (text, bad) in [(EN, "install"), (DE, "install")] {
        let line = text
            .lines()
            .find_map(|l| l.strip_prefix("plugin-manager.code.unsigned = "))
            .unwrap();
        assert!(line.len() <= 40, "{line}");
        assert!(!line.to_lowercase().contains(bad), "{line}");
    }
}

/// *How it works* (a held row) reads `<id>.plugin.how`: every plugin with
/// code that has a `how/` text has the key in its English and German
/// catalogs (#459: eleven of them had only the files, and the manager said
/// they had none).
#[test]
fn every_plugin_with_code_and_a_how_text_has_it_in_its_catalogs() {
    let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut checked = 0;
    for entry in std::fs::read_dir(&plugins).unwrap() {
        let dir = entry.unwrap().path();
        let id = dir.file_name().unwrap().to_string_lossy().into_owned();
        if !dir.join("src/native_plugin.rs").exists() || !dir.join("how/en.txt").exists() {
            continue;
        }
        for language in ["en", "de"] {
            let source = std::fs::read_to_string(dir.join(format!("i18n/{language}.ini"))).unwrap();
            let catalog = Catalog::parse(source).unwrap();
            let key = format!("{id}.plugin.how");
            let how = catalog.get(&key).unwrap_or_default();
            assert!(how.len() > 200, "{key} ({language}): {how:?}");
        }
        checked += 1;
    }
    assert!(checked >= 11, "{checked}");
}
