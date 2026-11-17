//! How the registries answer: hyphenation by language with its fallbacks,
//! services by type, and formats by extension. (What may register, the rules of the
//! kernel's `Items`, is tested in `paperpaw-core/tests/items_rules.rs`.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::borrow::Cow;

use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::services::{Service, Services};
use paperpaw_sdk::{
    FormatSpec, Formats, Hyphenation, HyphenationSpec, Id, Manifest, Priority, RegisterError,
    Version,
};

fn patterns(language: &str) -> &'static [u8] {
    // Patterns that differ by language: each breaks a word at another
    // place, so the one chosen is seen.
    let text = match language {
        "de" => "a1b",
        "de-AT" => "b1c",
        _ => "c1d",
    };
    Box::leak(paperpaw_sdk::hyphen::compile(text, "", 1, 1).into_boxed_slice())
}

fn breaks(hyphenation: &Hyphenation, language: &str, word: &str) -> Option<Vec<usize>> {
    let patterns = hyphenation.get(language)?;
    let mut at = Vec::new();
    patterns.hyphenate(word, &mut |i| at.push(i));
    Some(at)
}

#[test]
fn hyphenation_falls_back_to_the_base_language_and_back() {
    let mut hyphenation = Hyphenation::new();
    let spec = |id: &'static str, language: &'static str| {
        HyphenationSpec::new(Id::new_static(id), language, patterns(language))
    };
    hyphenation.add(spec("h.de", "de")).unwrap();
    hyphenation.add(spec("h.de-at", "de-AT")).unwrap();
    hyphenation.add(spec("h.en-us", "en-US")).unwrap();
    assert!(matches!(
        hyphenation.add(spec("h.de", "fr")),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(hyphenation.contains("h.de-at") && !hyphenation.contains("h.fr"));
    let de = breaks(&hyphenation, "de", "abcd");
    let at = breaks(&hyphenation, "de-AT", "abcd");
    let en = breaks(&hyphenation, "en-US", "abcd");
    assert_ne!(de, at);
    assert_ne!(de, en);
    // Case is ignored; a region not known takes the base language.
    assert_eq!(breaks(&hyphenation, "DE-at", "abcd"), at);
    assert_eq!(breaks(&hyphenation, "de-CH", "abcd"), de);
    assert_eq!(breaks(&hyphenation, "de_CH", "abcd"), de);
    // The base language takes a region's patterns where it has none.
    assert_eq!(breaks(&hyphenation, "en", "abcd"), en);
    assert_eq!(breaks(&hyphenation, "en-GB", "abcd"), en);
    assert_eq!(breaks(&hyphenation, "fr", "abcd"), None);
    assert_eq!(breaks(&hyphenation, "", "abcd"), None);
    // Patterns that are no patterns are refused.
    let broken = HyphenationSpec::new(Id::new_static("h.broken"), "xx", b"not compiled patterns");
    assert!(matches!(
        hyphenation.add(broken),
        Err(RegisterError::Invalid { .. })
    ));
    assert!(!hyphenation.contains("h.broken"));
}

#[test]
fn later_hyphenation_of_a_language_wins() {
    let mut hyphenation = Hyphenation::new();
    for (id, text) in [("h.one", "a1b"), ("h.two", "b1c")] {
        hyphenation
            .add(HyphenationSpec::new(
                Id::parse(id).unwrap(),
                "de",
                Box::leak(paperpaw_sdk::hyphen::compile(text, "", 1, 1).into_boxed_slice()),
            ))
            .unwrap();
    }
    let mut only_two = Hyphenation::new();
    only_two
        .add(HyphenationSpec::new(
            Id::new_static("h.two"),
            "de",
            Box::leak(paperpaw_sdk::hyphen::compile("b1c", "", 1, 1).into_boxed_slice()),
        ))
        .unwrap();
    assert_eq!(
        breaks(&hyphenation, "de", "abcd"),
        breaks(&only_two, "de", "abcd")
    );
}

#[test]
fn services_are_found_by_id_and_type_and_the_first_wins() {
    let mut services = Services::new();
    services.insert(Id::new_static("a.count"), Service::new(Count(1), ONE));
    services.insert(Id::new_static("a.count"), Service::new(Count(2), ONE));
    services.insert(
        Id::new_static("a.name"),
        Service::new(Name(String::from("x")), ONE),
    );
    assert_eq!(services.get::<Count>("a.count", ONE), Some(&Count(1)));
    assert_eq!(
        services.get::<Name>("a.name", ONE).map(|n| n.0.as_str()),
        Some("x")
    );
    assert_eq!(services.get::<Count>("a.name", ONE), None);
    assert!(!services.has("a.none"));
    services.contribute(
        Id::new_static("a.point"),
        Id::new_static("b"),
        Cow::Borrowed("1"),
    );
    services.contribute(
        Id::new_static("a.point"),
        Id::new_static("c"),
        Cow::Borrowed("2"),
    );
    let from: Vec<(&str, &str)> = services
        .contributions("a.point")
        .map(|(plugin, text)| (plugin.as_str(), text))
        .collect();
    assert_eq!(from, [("b", "1"), ("c", "2")]);
    assert_eq!(services.contributions("a.none").count(), 0);
    services.shrink_to_fit();
    assert!(format!("{services:?}").contains("a.count"));
}

fn format(id: &'static str, extensions: &[&'static str]) -> FormatSpec {
    FormatSpec::new(
        Id::new_static(id),
        "",
        extensions.iter().map(|e| Cow::Borrowed(*e)).collect(),
        Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
    )
}

#[test]
fn formats_are_found_by_the_longest_extension_and_later_wins() {
    let mut formats = Formats::new();
    formats.add(format("f.zip", &["zip"])).unwrap();
    formats.add(format("f.fb2", &["fb2", "fb2.zip"])).unwrap();
    formats.add(format("f.epub", &["epub"])).unwrap();
    formats.add(format("f.epub2", &["EPUB"])).unwrap();
    assert!(matches!(
        formats.add(format("f.zip", &["x"])),
        Err(RegisterError::Duplicate { .. })
    ));
    let id = |path: &str| formats.for_path(path).map(|f| f.id.as_str());
    assert_eq!(id("a/book.fb2.zip"), Some("f.fb2"));
    assert_eq!(id("a/book.zip"), Some("f.zip"));
    assert_eq!(id("Book.EpUb"), Some("f.epub2"));
    // The extension is the name's, after a dot, not the folder's.
    assert_eq!(id("a.epub/book"), None);
    assert_eq!(id("epub"), None);
    assert_eq!(id("bookepub"), None);
    assert_eq!(id("ünïcödé.epub"), Some("f.epub2"));
    assert_eq!(id(""), None);
    assert!(formats.supports("x.fb2") && !formats.supports("x.pdf"));
    assert!(formats.contains("f.zip") && !formats.contains("f.pdf"));
    // Opening what no format reads says so.
    let storage = paperpaw_sdk::MemoryStorage::new();
    assert!(matches!(
        formats.open(&storage, "x.pdf"),
        Err(DocumentError::UnsupportedFormat)
    ));
    assert!(formats.prepared(&storage, "x.pdf", &formats.options()));
}

#[test]
fn register_errors_say_what_is_wrong() {
    let id = Id::new_static("a.b");
    assert_eq!(
        RegisterError::Namespace { id: id.clone() }.to_string(),
        "`a.b` is outside the plugin's namespace"
    );
    assert_eq!(
        RegisterError::Duplicate { id }.to_string(),
        "`a.b` is already registered"
    );
    assert_eq!(
        RegisterError::Parse {
            line: 3,
            message: Cow::Borrowed("bad")
        }
        .to_string(),
        "line 3: bad"
    );
    assert_eq!(
        RegisterError::Invalid {
            message: Cow::Borrowed("why")
        }
        .to_string(),
        "why"
    );
    assert!(Priority::FALLBACK < Priority::DEFAULT && Priority::DEFAULT < Priority::PREFERRED);
    let manifest = Manifest::new(Id::new_static("demo"), "Demo", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING);
    assert_eq!(manifest.clone(), manifest);
}

/// A service of a count, for the tests.
#[derive(Debug, PartialEq)]
struct Count(u32);
paperpaw_sdk::service_type!(Count, "demo.count/1");

/// A service of a name, for the tests.
#[derive(Debug, PartialEq)]
struct Name(String);
paperpaw_sdk::service_type!(Name, "demo.name/1");

/// The version the test services are in.
const ONE: paperpaw_sdk::Version = paperpaw_sdk::Version::new(1, 0, 0);
