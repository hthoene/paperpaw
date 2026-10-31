//! Catalogs, plural rules and placeholders at their edges: every plural
//! rule against the CLDR examples for its languages, broken and unusual
//! catalogs, translations with missing or extra placeholders, fallbacks
//! between plural forms and languages, arguments that look like
//! placeholders. Properties: any text parses into a catalog or an error
//! without panicking; generated catalogs give every message as the
//! configuration parser reads it, also after unloading and loading.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;

use paperpaw_i18n::check::{Issue, compare};
use paperpaw_i18n::{Arg, Catalog, CatalogError, PluralCategory, PluralRule, Translator};
use proptest::prelude::*;

use PluralCategory::{Few, Many, One, Other};

/// Checks `rule` against CLDR's integer examples, `(category, counts)`.
fn check_rule(rule: PluralRule, examples: &[(PluralCategory, &[i64])]) {
    for (category, counts) in examples {
        for &count in *counts {
            assert_eq!(rule.category(count), *category, "{} {count}", rule.id());
            // CLDR's operands are absolute values.
            assert_eq!(rule.category(-count), *category, "{} -{count}", rule.id());
        }
    }
}

#[test]
fn one_other_matches_cldr_for_english_and_german() {
    check_rule(
        PluralRule::OneOther,
        &[
            (One, &[1]),
            (
                Other,
                &[
                    0, 2, 3, 10, 11, 16, 21, 100, 101, 1000, 10_000, 100_000, 1_000_000,
                ],
            ),
        ],
    );
}

#[test]
fn zero_one_other_matches_cldr_for_french() {
    // CLDR's "many" for French (a million and its multiples) is not part
    // of this rule; those counts read "other".
    check_rule(
        PluralRule::ZeroOneOther,
        &[
            (One, &[0, 1]),
            (
                Other,
                &[2, 3, 10, 11, 17, 21, 100, 101, 1000, 10_000, 100_000],
            ),
        ],
    );
}

#[test]
fn east_slavic_matches_cldr_for_russian() {
    check_rule(
        PluralRule::EastSlavic,
        &[
            (One, &[1, 21, 31, 41, 51, 61, 71, 81, 101, 1001, 100_021]),
            (
                Few,
                &[2, 3, 4, 22, 23, 24, 32, 33, 34, 42, 52, 62, 102, 1002],
            ),
            (
                Many,
                &[
                    0, 5, 6, 9, 10, 11, 12, 13, 14, 15, 19, 20, 25, 100, 111, 112, 114, 1000,
                    10_000, 100_000, 1_000_000,
                ],
            ),
        ],
    );
}

#[test]
fn polish_matches_cldr() {
    check_rule(
        PluralRule::Polish,
        &[
            (One, &[1]),
            (
                Few,
                &[2, 3, 4, 22, 23, 24, 32, 33, 34, 42, 52, 62, 102, 1002],
            ),
            (
                Many,
                &[
                    0, 5, 6, 10, 11, 12, 13, 14, 19, 20, 21, 31, 101, 112, 1000, 10_000, 100_000,
                    1_000_000,
                ],
            ),
        ],
    );
}

#[test]
fn czech_slovak_matches_cldr() {
    // CLDR's "many" is only for fractions; integers are one, few, other.
    check_rule(
        PluralRule::CzechSlovak,
        &[
            (One, &[1]),
            (Few, &[2, 3, 4]),
            (
                Other,
                &[
                    0, 5, 6, 10, 19, 21, 22, 24, 100, 102, 1000, 10_000, 1_000_000,
                ],
            ),
        ],
    );
}

#[test]
fn other_has_one_form_for_every_count() {
    check_rule(PluralRule::Other, &[(Other, &[0, 1, 2, 5, 11, 21, 101])]);
}

#[test]
fn extreme_counts_are_categorised() {
    for rule in [
        "other",
        "one-other",
        "zero-one-other",
        "east-slavic",
        "polish",
        "czech-slovak",
    ] {
        let rule = PluralRule::from_id(rule).unwrap();
        // i64::MIN has no positive counterpart; it must still work.
        let _ = rule.category(i64::MIN);
        let _ = rule.category(i64::MAX);
    }
    // ...808 ends in 08, ...807 in 07: both "many" in Russian.
    assert_eq!(PluralRule::EastSlavic.category(i64::MIN), Many);
    assert_eq!(PluralRule::EastSlavic.category(i64::MAX), Many);
}

#[test]
fn plural_rule_ids_are_exact() {
    for id in [
        "",
        "One-Other",
        "one_other",
        "one-other ",
        "east slavic",
        "ru",
    ] {
        assert_eq!(PluralRule::from_id(id), None, "{id:?}");
    }
}

fn catalog(source: &str) -> Catalog {
    Catalog::parse(source.to_string()).unwrap()
}

#[test]
fn catalogs_that_are_not_well_formed() {
    let err = |source: &'static str| Catalog::parse(source).unwrap_err();
    assert_eq!(err(""), CatalogError::MissingLanguage);
    assert_eq!(err("# only a comment\n"), CatalogError::MissingLanguage);
    assert_eq!(
        err("key = before any section\n[meta]\nlanguage = en\n"),
        CatalogError::UnexpectedSection { line: 1 }
    );
    match err("[meta]\nlanguage = en\n[messages]\nbroken line\n") {
        CatalogError::Syntax(e) => assert_eq!(e.line, 4),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("[meta]\nlanguage = en\nplural = \"one-other \"\n"),
        CatalogError::UnknownPluralRule { line: 3 }
    );
    // Errors are displayed with their line.
    assert_eq!(
        err("a = b\n[meta]\nlanguage = en\n").to_string(),
        "line 1: a key before [meta]"
    );
}

#[test]
fn meta_may_come_last_and_be_given_twice() {
    let c = catalog(
        "[messages]\na = \"x\\ty\"\nb = plain\n[meta]\nname = First\nlanguage = de\n\
         [meta]\nname = \"Zweiter \\\"Name\\\"\"\nplural = polish\n",
    );
    assert_eq!(c.language(), "de");
    assert_eq!(c.name(), "Zweiter \"Name\"");
    assert_eq!(c.plural_rule(), PluralRule::Polish);
    assert_eq!(c.get("a"), Some("x\ty"));
    let mut c = c;
    c.unload();
    assert!(!c.is_loaded());
    assert_eq!(
        (c.language(), c.name(), c.get("a")),
        ("de", "Zweiter \"Name\"", None)
    );
    c.load();
    assert_eq!((c.get("a"), c.get("b")), (Some("x\ty"), Some("plain")));
    // Loading twice changes nothing.
    c.load();
    assert_eq!(c.len(), 2);
}

#[test]
fn keys_with_marks_and_tricky_values_are_found() {
    let c = catalog(
        "[meta]\nlanguage = en\n[messages]\n\
         clock@2.face = Face\n\
         a.b-c_d = dotted\n\
         spaced   =   kept = inside  \n\
         \tindented=\tvalue\n\
         empty =\n\
         quote = \"\"\n\
         unicode = \"\\u{1F4D6}\"\n\
         crlf = ends\r\n",
    );
    assert_eq!(c.get("clock@2.face"), Some("Face"));
    assert_eq!(c.get("clock"), None);
    assert_eq!(c.get("a.b-c_d"), Some("dotted"));
    assert_eq!(c.get("spaced"), Some("kept = inside"));
    assert_eq!(c.get("indented"), Some("value"));
    assert_eq!(c.get("empty"), Some(""));
    assert_eq!(c.get("quote"), Some(""));
    assert_eq!(c.get("unicode"), Some("\u{1f4d6}"));
    assert_eq!(c.get("crlf"), Some("ends"));
    let keys: Vec<&str> = c.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
    assert_eq!(keys.len(), 8);
}

const EN: &str = "[meta]\nlanguage = en\n[messages]\n\
    hello = Hello, {name}!\nbraces = {{literal}} and {name}\nplain = Plain\n\
    items.one = {count} item\nitems.other = {count} items\n\
    files.one = one file\nfiles.other = {count} files\n";

#[test]
fn missing_extra_and_renamed_placeholders_are_found() {
    let reference = catalog(EN);
    let translation = catalog(
        "[meta]\nlanguage = de\n[messages]\n\
         hello = Hallo!\nbraces = {{wörtlich}} und {name} {extra}\nplain = {neu}\n\
         items.one = {count} Ding\nitems.other = {anzahl} Dinge\n\
         files.one = eine Datei\nfiles.other = {count} Dateien\n",
    );
    let issues = compare(&reference, &translation);
    let mut placeholders: Vec<String> = issues
        .iter()
        .filter_map(|issue| match issue {
            Issue::Placeholders(key) => Some(key.clone()),
            _ => None,
        })
        .collect();
    placeholders.sort();
    assert_eq!(placeholders, ["braces", "hello", "items.other", "plain"]);
    assert_eq!(issues.len(), 4, "{issues:?}");
}

#[test]
fn plural_forms_are_compared_by_their_base_key() {
    let reference = catalog(EN);
    // Russian has few and many where English has other; a form missing
    // or added is no issue, only a message missing altogether.
    let translation = catalog(
        "[meta]\nlanguage = ru\nplural = east-slavic\n[messages]\n\
         hello = Привет, {name}!\nbraces = {{x}} {name}\nplain = Просто\n\
         items.one = {count} предмет\nitems.few = {count} предмета\nitems.many = {count} предметов\n\
         files.many = {count} файлов\n",
    );
    assert!(compare(&reference, &translation).is_empty());
    let lacking = catalog("[meta]\nlanguage = ru\n[messages]\nhello = {name}\nextra.few = x\n");
    let issues = compare(&reference, &lacking);
    for missing in ["braces", "plain", "items", "files"] {
        assert!(
            issues.contains(&Issue::Missing(missing.into())),
            "{missing}"
        );
    }
    assert!(issues.contains(&Issue::Unknown("extra".into())));
    assert_eq!(
        Issue::Placeholders("k".into()).to_string(),
        "placeholders of `k` differ"
    );
}

fn translator(sources: &[&str], language: &str) -> Translator {
    let mut t = Translator::new();
    for source in sources {
        t.add(catalog(source));
    }
    t.set_language(language);
    t
}

#[test]
fn plural_forms_fall_back_within_a_catalog_before_the_next_language() {
    let ru = "[meta]\nlanguage = ru\nplural = east-slavic\n[messages]\n\
              items.one = {count} предмет\nitems.few = {count} предмета\nitems.other = {count} шт.\n";
    let t = translator(&[EN, ru], "ru");
    assert_eq!(t.plural("items", 1, &[]), "1 предмет");
    assert_eq!(t.plural("items", 22, &[]), "22 предмета");
    // No "many" in Russian: its own "other", not English.
    assert_eq!(t.plural("items", 5, &[]), "5 шт.");
    assert_eq!(t.plural("items", -21, &[]), "-21 предмет");
    // Not in Russian at all: English with English's rule.
    assert_eq!(t.plural("files", 1, &[]), "one file");
    assert_eq!(t.plural("files", 3, &[]), "3 files");
    assert_eq!(t.plural("missing", 3, &[]), "missing");
    assert_eq!(t.plural("items", i64::MIN, &[]), "-9223372036854775808 шт.");
}

#[test]
fn arguments_are_not_read_as_placeholders() {
    let t = translator(&[EN], "en");
    // A value that looks like a placeholder is put in as it is.
    assert_eq!(
        t.format("hello", &[("name", Arg::Str("{name}{{x}}"))]),
        "Hello, {name}{{x}}!"
    );
    assert_eq!(
        t.format("braces", &[("name", Arg::Int(-7))]),
        "{literal} and -7"
    );
    // Missing arguments stay visible; unknown keys come back as they are.
    assert_eq!(t.format("hello", &[]), "Hello, {name}!");
    assert_eq!(
        t.format("no.such.key {x}", &[("x", Arg::Int(1))]),
        "no.such.key 1"
    );
    assert_eq!(t.tr("{}"), "{}");
}

#[test]
fn languages_without_catalogs_fall_back_to_english() {
    for language in ["", "-", "xx", "de-", "-de", "DE", "en-US"] {
        let t = translator(&[EN], language);
        assert_eq!(t.tr("plain"), "Plain", "{language:?}");
        assert_eq!(t.language(), language);
    }
}

#[test]
fn many_catalogs_and_messages() {
    let mut t = Translator::new();
    for i in 0..50 {
        let messages: String = (0..200)
            .map(|m| format!("plugin{i}.message{m} = Text {i} {m}\n"))
            .collect();
        t.add(catalog(&format!(
            "[meta]\nlanguage = en\n[messages]\n{messages}"
        )));
    }
    t.shrink_to_fit();
    assert_eq!(t.tr("plugin49.message199"), "Text 49 199");
    assert_eq!(t.tr("plugin0.message0"), "Text 0 0");
    assert_eq!(t.tr("plugin50.message0"), "plugin50.message0");
}

/// A line of a generated catalog.
#[derive(Debug, Clone)]
enum Line {
    Message(String, String, u8),
    Comment(String),
    Blank,
}

fn line() -> impl Strategy<Value = Line> {
    let key = prop::sample::select(vec![
        "a",
        "b",
        "a.b",
        "home.continue",
        "x-y_z",
        "clock@2.face",
        "n1",
        "Z",
    ]);
    prop_oneof![
        6 => (key, "[^\\r]{0,30}", 0u8..4)
            .prop_map(|(k, v, style)| Line::Message(k.to_string(), v, style)),
        1 => "[^\\r\\n]{0,20}".prop_map(|c| Line::Comment(format!("# {c}"))),
        1 => Just(Line::Blank),
    ]
}

fn write(lines: &[Line], crlf: bool) -> String {
    let newline = if crlf { "\r\n" } else { "\n" };
    let mut out = format!("[meta]{newline}language = en{newline}[messages]{newline}");
    for line in lines {
        match line {
            Line::Message(key, value, style) => {
                let value = paperpaw_config::quote(value);
                match style {
                    0 => out.push_str(&format!("{key} = {value}")),
                    1 => out.push_str(&format!("{key}={value}")),
                    2 => out.push_str(&format!("  {key}\t=   {value} \t")),
                    _ => out.push_str(&format!("\t{key} ={value}")),
                }
            }
            Line::Comment(c) => out.push_str(c),
            Line::Blank => {}
        }
        out.push_str(newline);
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn any_text_parses_or_fails_without_panicking(
        body in "\\PC{0,200}|[\\[\\]=\"\\\\u{}#;@. \\na-z]{0,200}",
        meta_first in any::<bool>(),
    ) {
        let source = if meta_first {
            format!("[meta]\nlanguage = en\n[messages]\n{body}")
        } else {
            body
        };
        if let Ok(mut c) = Catalog::parse(source) {
            let keys: Vec<String> = c.keys().map(str::to_string).collect();
            for key in &keys {
                prop_assert!(c.get(key).is_some(), "{key}");
            }
            c.unload();
            c.load();
            prop_assert_eq!(c.keys().map(str::to_string).collect::<Vec<_>>(), keys);
        }
    }

    #[test]
    fn generated_catalogs_give_every_message(
        lines in proptest::collection::vec(line(), 0..30),
        crlf in any::<bool>(),
        owned in any::<bool>(),
    ) {
        let source = write(&lines, crlf);
        let mut model: BTreeMap<String, String> = BTreeMap::new();
        for line in &lines {
            if let Line::Message(key, value, _) = line {
                model.insert(key.clone(), value.clone());
            }
        }
        let mut c = if owned {
            Catalog::parse(source.clone()).unwrap()
        } else {
            Catalog::parse(&*Box::leak(source.clone().into_boxed_str())).unwrap()
        };
        let check = |c: &Catalog| -> Result<(), TestCaseError> {
            prop_assert_eq!(c.len(), model.len());
            prop_assert!(c.keys().eq(model.keys().map(String::as_str)));
            for (key, value) in &model {
                prop_assert_eq!(c.get(key), Some(value.as_str()), "{}", key);
            }
            prop_assert_eq!(c.get("not.there"), None);
            Ok(())
        };
        check(&c)?;
        c.unload();
        prop_assert!(c.is_empty());
        c.load();
        check(&c)?;
        let mut t = Translator::new();
        t.add(c);
        for (key, value) in &model {
            prop_assert_eq!(t.tr(key), value.as_str());
        }
    }

    #[test]
    fn categories_stay_within_each_rule(count in any::<i64>()) {
        use PluralCategory::Zero;
        let allowed: [(PluralRule, &[PluralCategory]); 6] = [
            (PluralRule::Other, &[Other]),
            (PluralRule::OneOther, &[One, Other]),
            (PluralRule::ZeroOneOther, &[One, Other]),
            (PluralRule::EastSlavic, &[One, Few, Many]),
            (PluralRule::Polish, &[One, Few, Many]),
            (PluralRule::CzechSlovak, &[One, Few, Other]),
        ];
        for (rule, categories) in allowed {
            let category = rule.category(count);
            prop_assert!(categories.contains(&category), "{} {count}: {category:?}", rule.id());
            prop_assert_ne!(category, Zero);
            prop_assert_eq!(rule.category(count.saturating_neg()), rule.category(count.saturating_abs()));
        }
        // Above a hundred, only the last two digits matter.
        if count >= 100 {
            let small = count % 100 + 100;
            prop_assert_eq!(PluralRule::EastSlavic.category(count), PluralRule::EastSlavic.category(small));
            prop_assert_eq!(PluralRule::Polish.category(count), PluralRule::Polish.category(small));
        }
    }

    #[test]
    fn values_without_braces_are_put_in_unchanged(
        before in "[^{}]{0,20}",
        after in "[^{}]{0,20}",
        value in "\\PC{0,20}",
    ) {
        let source = format!(
            "[meta]\nlanguage = en\n[messages]\nm = {}\n",
            paperpaw_config::quote(&format!("{before}{{v}}{after}"))
        );
        prop_assume!(!before.contains('\r') && !after.contains('\r'));
        let t = translator(&[&source], "en");
        prop_assert_eq!(
            t.format("m", &[("v", Arg::Str(&value))]),
            format!("{before}{value}{after}")
        );
    }
}
