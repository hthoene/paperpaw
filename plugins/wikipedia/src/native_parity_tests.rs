//! The native lookup is the interface-6 lookup where they overlap: the
//! screen's and the provider's ids and titles, the provider's network flag,
//! and the addresses and the summary read.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use crate::{
    PLUGIN, PROVIDER, SCREEN, Wikipedia, article_url, declarations,
    native_model::{self, Outcome},
    summary_url, wiki,
};
use alloc::{string::String, vec::Vec};
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::{Contribution, Plugin, Registrar, native::declare::Declarations};

fn batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::screen(&mut d).unwrap();
        declarations::provider(&mut d).unwrap();
    }
    out
}

#[test]
fn the_screen_and_the_provider_are_the_ones_interface_6_registers() {
    let mut r = Registrar::new(PLUGIN);
    Wikipedia.register(&mut r).unwrap();
    let (mut screens, mut providers) = (Vec::new(), Vec::new());
    for c in r.into_contributions() {
        match c {
            Contribution::FixedScreens(list) => {
                for s in list {
                    screens.push((String::from(s.id().as_str()), String::from(s.title())));
                }
            }
            Contribution::LookupProvider(p) => {
                providers.push((
                    String::from(p.id.as_str()),
                    String::from(&*p.title),
                    p.online,
                ));
            }
            _ => {}
        }
    }
    assert_eq!(
        screens,
        [(String::from(SCREEN.as_str()), "wikipedia.title".into())]
    );
    assert_eq!(
        providers,
        [(
            String::from(PROVIDER.as_str()),
            "wikipedia.title".into(),
            true
        )]
    );
    let built = batches();
    assert_eq!(built.len(), 2);
    let mut seen = Vec::new();
    let records: Vec<_> = built
        .iter()
        .flat_map(|batch| records_decode(batch).unwrap().iter().collect::<Vec<_>>())
        .collect();
    for record in records {
        match record {
            Record::ScreenSpec(s) => {
                seen.push(alloc::format!("screen {} {}", s.name, s.title));
            }
            Record::LookupSpec(p) => {
                seen.push(alloc::format!("lookup {} {} {}", p.name, p.title, p.flags));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        seen,
        [
            "screen lookup wikipedia.title",
            "lookup summary wikipedia.title 1"
        ]
    );
}

#[test]
fn the_addresses_are_the_interface_6_addresses() {
    for language in ["en", "de-DE", "pt_BR", "", "1234", "EN-gb", "zh-Hans", "x"] {
        assert_eq!(native_model::wiki(language).unwrap(), wiki(language));
        for word in [
            "Hund",
            "white rabbit",
            " padded ",
            "a+b",
            "Ärger & Öl/x",
            "",
            "日本",
        ] {
            assert_eq!(
                native_model::summary_url(language, word).unwrap(),
                summary_url(language, word),
                "{language} {word}"
            );
            assert_eq!(
                native_model::article_url(language, word).unwrap(),
                article_url(language, word),
                "{language} {word}"
            );
        }
    }
}

#[test]
fn the_summary_read_is_the_interface_6_summary() {
    let bodies = [
        r#"{"type":"standard","title":"Hund","extract":"Der Hund ist ein Haustier.","k":1}"#,
        r#"{"title":"A \"quoted\" ä name","extract":"Line\nbreak"}"#,
        r#"{"title":"No extract"}"#,
        r#"{"extract":"   "}"#,
        r#"{"extract":null,"title":"T"}"#,
        r#"{"extract":"x","extract":"y"}"#,
        r#"{"nested":{"extract":"inner"},"extract":"outer"}"#,
        "not json at all",
        "",
    ];
    for body in bodies {
        let members = paperpaw_sdk::json::object(body);
        let want_text = paperpaw_sdk::json::text(&members, "extract")
            .filter(|t| !t.trim().is_empty())
            .map(String::from);
        let want_title = paperpaw_sdk::json::text(&members, "title").map(String::from);
        match native_model::outcome(200, body).unwrap() {
            Outcome::Found { title, text } => {
                assert_eq!(Some(text), want_text, "{body}");
                assert_eq!(title, want_title, "{body}");
            }
            Outcome::NotFound => assert_eq!(want_text, None, "{body}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn the_statuses_say_what_interface_6_says() {
    let body = r#"{"extract":"x"}"#;
    assert!(matches!(
        native_model::outcome(299, body),
        Ok(Outcome::Found { .. })
    ));
    assert_eq!(native_model::outcome(404, body), Ok(Outcome::NotFound));
    assert_eq!(native_model::outcome(0, ""), Ok(Outcome::Unreachable));
    assert_eq!(native_model::outcome(500, body), Ok(Outcome::Failed));
    assert_eq!(native_model::outcome(301, body), Ok(Outcome::Failed));
    let long = alloc::format!(r#"{{"extract":"{}"}}"#, "x".repeat(native_model::MAX_BODY));
    assert_eq!(
        native_model::outcome(200, &long),
        Err(paperpaw_sdk::native::Error::NoSpace)
    );
}
