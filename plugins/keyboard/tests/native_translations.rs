//! The plugin's catalogs: German says everything English does, with the same
//! placeholders (the interface-6 `translations_are_complete`, now over the
//! native plugin's catalogs, which are unchanged).
#![allow(clippy::unwrap_used, reason = "test assertions")]
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
