//! The English and German texts of the plugin say the same keys.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_i18n::{Catalog, check};

#[test]
fn translations_are_complete() {
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}
