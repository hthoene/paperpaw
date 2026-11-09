use paperpaw_i18n::{Catalog, check};

#[test]
fn translations_are_complete() {
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

/// The page of *Your own fonts* is the help (`how/`) from after its heading
/// on, kept as a message so that the native plugin needs no help table.
#[test]
fn the_text_of_your_own_fonts_is_the_help_after_its_heading() {
    for (ini, how) in [
        (include_str!("../i18n/en.ini"), include_str!("../how/en.txt")),
        (include_str!("../i18n/de.ini"), include_str!("../how/de.txt")),
    ] {
        let catalog = Catalog::parse(ini).unwrap();
        let heading = catalog.get("fonts-browser.own.heading").unwrap();
        let at = how.find(heading).unwrap();
        let text = how[at + heading.len()..].trim();
        assert_eq!(catalog.get("fonts-browser.own.text"), Some(text));
    }
}
