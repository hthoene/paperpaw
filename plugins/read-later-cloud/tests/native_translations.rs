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
    for part in include_str!("../src/native_plugin.rs")
        .split('"')
        .skip(1)
        .step_by(2)
    {
        let text = part.starts_with("read-later-cloud.")
            && !part.contains(' ')
            && part != "read-later-cloud.setup";
        if text {
            asked += 1;
            for (name, catalog) in [("en", EN), ("de", DE)] {
                assert!(has(catalog, part), "{part} missing in {name}");
            }
        }
    }
    assert!(asked >= 8, "{asked}");
}
