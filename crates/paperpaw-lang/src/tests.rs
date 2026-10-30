use super::*;

#[test]
fn tags_give_their_parts() {
    let fa = Lang::parse("fa-Arab-IR");
    assert_eq!(fa.language().as_str(), "fa");
    assert_eq!(fa.script(), Some("Arab"));
    assert_eq!(fa.region(), Some("IR"));
    let es = Lang::parse("es-419");
    assert_eq!(es.region(), Some("419"));
    assert_eq!(es.script(), None);
    assert_eq!(Lang::parse("ckb").language().as_str(), "ckb");
}

#[test]
fn one_language_has_one_value() {
    for (a, b) in [
        ("ar", "ara"),
        ("fa", "per"),
        ("fa", "fas"),
        ("he", "iw"),
        ("id", "in"),
        ("yi", "ji"),
        ("de", "ger"),
        ("de", "DEU"),
        ("sr-Latn", "srp-latn"),
        ("pt-BR", "pt_br"),
    ] {
        assert_eq!(Lang::parse(a), Lang::parse(b), "{a} {b}");
        assert!(!Lang::parse(a).is_none());
    }
}

#[test]
fn what_is_not_a_tag_is_none() {
    for tag in ["", "a", "abcd", "12", "ä", "-", "en!"] {
        assert!(Lang::parse(tag).is_none(), "{tag}");
    }
}

#[test]
fn variants_and_extensions_are_dropped() {
    assert_eq!(Lang::parse("de-DE-1996"), Lang::parse("de-DE"));
    assert_eq!(Lang::parse("en-US-x-twain"), Lang::parse("en-US"));
}

#[test]
fn bits_go_and_come_back() {
    let lang = Lang::parse("zh-Hant-TW");
    assert_eq!(Lang::from_bits(lang.to_bits()), Some(lang));
    assert_eq!(Lang::from_bits(u32::MAX), None);
}

#[test]
fn a_script_is_added_only_where_none_is() {
    assert_eq!(Lang::parse("ur").or_script("arab").script(), Some("Arab"));
    assert_eq!(
        Lang::parse("sr-Cyrl").or_script("Latn").script(),
        Some("Cyrl")
    );
}

#[test]
fn the_tables_fit_their_bits() {
    assert!(tables::SCRIPTS.len() < 256);
    assert!(tables::REGIONS.len() < 512);
    assert!(tables::CANONICAL.windows(2).all(|w| w[0].0 < w[1].0));
}

proptest::proptest! {
    #[test]
    fn any_text_parses_without_panic(tag in "\\PC{0,24}") {
        let lang = Lang::parse(&tag);
        let _ = (lang.language(), lang.script(), lang.region());
    }
}
