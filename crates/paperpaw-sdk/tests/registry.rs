//! How the registries answer: input and refresh filters in their order,
//! filter signatures, hyphenation by language with its fallbacks, services
//! by type, and formats by extension. (What may register, the rules of the
//! kernel's `Items`, is tested in `paperpaw-core/tests/items_rules.rs`.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::borrow::Cow;

use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::filters::{
    InputFilterSpec, InputFilters, InputVerdict, Refresh, RefreshCause, RefreshFilterSpec,
    RefreshFilters, RefreshVerdict, TextFilterSpec, TextFilters,
};
use paperpaw_sdk::services::{Service, Services};
use paperpaw_sdk::{
    ButtonState, FormatSpec, Formats, Hyphenation, HyphenationSpec, Id, InputEvent, Manifest,
    Priority, Rect, RefreshMode, RegisterError, Version,
};

#[test]
fn input_filters_take_what_the_first_one_does_not_pass() {
    let mut filters = InputFilters::new();
    let button = |name: &'static str| InputEvent::Button {
        name,
        state: ButtonState::Pressed,
    };
    let make = |id: &'static str, verdict: fn(&InputEvent) -> InputVerdict| {
        InputFilterSpec::new(
            Id::new_static(id),
            "",
            Box::new(move |_, event| verdict(event)),
        )
    };
    let h = common::Harness::new();
    let env = h.env();
    assert_eq!(filters.apply(&env, &button("a")), InputVerdict::Pass);
    filters.add(make("f.lock", |e| match e {
        InputEvent::Button { name: "lock", .. } => InputVerdict::Take,
        _ => InputVerdict::Pass,
    }));
    filters.add(make("f.run", |e| match e {
        InputEvent::Button {
            name: "lock" | "run",
            ..
        } => InputVerdict::Run(Id::new_static("f.action")),
        _ => InputVerdict::Pass,
    }));
    filters.shrink_to_fit();
    assert_eq!(filters.apply(&env, &button("lock")), InputVerdict::Take);
    assert_eq!(
        filters.apply(&env, &button("run")),
        InputVerdict::Run(Id::new_static("f.action"))
    );
    assert_eq!(filters.apply(&env, &button("other")), InputVerdict::Pass);
    // A filter put first replaces an input: the filters after it see
    // the other one, and what comes out is the other one.
    let mut filters = InputFilters::new();
    filters.add(make("f.swap", |e| match e {
        InputEvent::Button { name: "swap", .. } => InputVerdict::Replace(InputEvent::Button {
            name: "run",
            state: ButtonState::Pressed,
        }),
        InputEvent::Button { name: "calm", .. } => InputVerdict::Replace(InputEvent::Button {
            name: "quiet",
            state: ButtonState::Pressed,
        }),
        _ => InputVerdict::Pass,
    }));
    filters.add(make("f.run", |e| match e {
        InputEvent::Button { name: "run", .. } => InputVerdict::Run(Id::new_static("f.action")),
        _ => InputVerdict::Pass,
    }));
    assert_eq!(
        filters.apply(&env, &button("swap")),
        InputVerdict::Run(Id::new_static("f.action"))
    );
    assert_eq!(
        filters.apply(&env, &button("calm")),
        InputVerdict::Replace(button("quiet"))
    );
}

#[test]
fn refresh_filters_change_the_mode_in_turn_and_full_covers_the_screen() {
    let screen = Rect {
        x: 0,
        y: 0,
        width: 480,
        height: 800,
    };
    let part = Rect {
        x: 10,
        y: 20,
        width: 30,
        height: 40,
    };
    let mut filters = RefreshFilters::new();
    let h = common::Harness::new();
    let env = h.env();
    let refresh = Refresh::new(
        RefreshMode::Partial,
        part,
        Some(Id::new_static("core.reader")),
    );
    assert_eq!(
        filters.apply(&env, refresh.clone(), screen),
        Some(refresh.clone())
    );
    filters.add(RefreshFilterSpec::new(
        Id::new_static("f.full"),
        "",
        Box::new(|_, r| {
            if r.screen
                .as_ref()
                .is_some_and(|s| s.as_str() == "core.reader")
            {
                RefreshVerdict::Mode(RefreshMode::Full)
            } else {
                RefreshVerdict::Keep
            }
        }),
    ));
    // A later filter sees the change and may take it back; the region
    // stays the whole screen.
    filters.add(RefreshFilterSpec::new(
        Id::new_static("f.fast"),
        "",
        Box::new(|_, r| match r.mode {
            RefreshMode::Full => RefreshVerdict::Mode(RefreshMode::Fast),
            _ => RefreshVerdict::Keep,
        }),
    ));
    let changed = filters.apply(&env, refresh.clone(), screen).unwrap();
    assert_eq!((changed.mode, changed.region), (RefreshMode::Fast, screen));
    let mut elsewhere = refresh.clone();
    elsewhere.screen = None;
    assert_eq!(
        filters.apply(&env, elsewhere.clone(), screen),
        Some(elsewhere.clone())
    );
    // A filter shows it in grays, another defers what a clock changed.
    filters.add(RefreshFilterSpec::new(
        Id::new_static("f.grays"),
        "",
        Box::new(|_, _| RefreshVerdict::Grays(true)),
    ));
    let grays = filters.apply(&env, elsewhere.clone(), screen).unwrap();
    assert!(grays.grays);
    filters.add(RefreshFilterSpec::new(
        Id::new_static("f.clock"),
        "",
        Box::new(|_, r| match r.cause {
            RefreshCause::Status => RefreshVerdict::Defer,
            _ => RefreshVerdict::Keep,
        }),
    ));
    elsewhere.cause = RefreshCause::Status;
    assert_eq!(filters.apply(&env, elsewhere, screen), None);
}

#[test]
fn text_filter_signatures_name_those_that_do_something() {
    let mut filters = TextFilters::new();
    let h = common::Harness::new();
    let env = h.env();
    assert_eq!(filters.signature(&env), 0);
    for (id, signature) in [("f.bold", 1), ("f.idle", 0), ("f.size", 3)] {
        filters.add(TextFilterSpec::new(
            Id::new_static(id),
            "",
            Box::new(|_, _| {}),
            Box::new(move |_| signature),
        ));
    }
    filters.shrink_to_fit();
    let mut expected = paperpaw_sdk::digest::Fnv1a64::new();
    for (id, part) in [("f.bold", 1u64), ("f.size", 3)] {
        expected.feed(id.as_bytes());
        expected.feed(&part.to_le_bytes());
    }
    assert_eq!(
        filters.signature(&env),
        expected.value(),
        "the idle one left out"
    );
    assert!(format!("{filters:?}").contains("f.bold"));
}

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
