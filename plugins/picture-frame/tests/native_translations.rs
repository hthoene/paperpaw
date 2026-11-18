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
        include_str!("../src/native_home.rs"),
        include_str!("../src/native_sync.rs"),
        include_str!("../declarations.rs"),
    ] {
        for part in source.split('"').skip(1).step_by(2) {
            // A key of this plugin: `picture-frame.` and a name, not an id of
            // a screen, an action or a menu entry, nor a setting or a path.
            let key = part.starts_with("picture-frame.")
                && !part.contains(' ')
                && !part.ends_with('.')
                && !matches!(
                    part,
                    "picture-frame.home"
                        | "picture-frame.show"
                        | "picture-frame.folder"
                        | "picture-frame.change"
                        | "picture-frame.fit"
                        | "picture-frame.contrast"
                        | "picture-frame.interval"
                        | "picture-frame.cache-mb"
                        | "picture-frame.slideshow-on-charge"
                        | "picture-frame.server"
                        | "picture-frame.user"
                        | "picture-frame.group"
                );
            if key {
                asked += 1;
                for (name, catalog) in [("en", EN), ("de", DE)] {
                    assert!(has(catalog, part), "{part} missing in {name}");
                }
            }
        }
    }
    assert!(asked >= 40, "{asked}");
}

#[test]
fn the_how_it_works_texts_explain_the_server_and_the_power_rule() {
    for (text, words) in [
        (
            include_str!("../how/en.txt"),
            ["WebDAV", "Wi-Fi", "Slideshow", "Photo of the day"],
        ),
        (
            include_str!("../how/de.txt"),
            ["WebDAV", "WLAN", "Diashow", "Foto des Tages"],
        ),
    ] {
        for word in words {
            assert!(text.contains(word), "{word}");
        }
    }
}
