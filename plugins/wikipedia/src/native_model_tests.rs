//! What the model answers: addresses and the summary read from an answer.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use crate::native_model::{self, Outcome};
use alloc::string::String;

#[test]
fn addresses() {
    assert_eq!(
        native_model::summary_url("de-DE", "Hund").unwrap(),
        "https://de.wikipedia.org/api/rest_v1/page/summary/Hund"
    );
    assert_eq!(
        native_model::summary_url("en", "white rabbit").unwrap(),
        "https://en.wikipedia.org/api/rest_v1/page/summary/white_rabbit"
    );
    assert_eq!(
        native_model::summary_url("en", "a+b").unwrap(),
        "https://en.wikipedia.org/api/rest_v1/page/summary/a%2Bb"
    );
    for (language, wiki) in [
        ("", "en"),
        ("pt_BR", "pt"),
        ("1234", "en"),
        ("EN-gb", "en"),
        ("zh-Hans", "zh"),
    ] {
        assert_eq!(native_model::wiki(language).unwrap(), wiki, "{language}");
    }
}

#[test]
fn the_summary_is_read_from_the_answer() {
    let found = |title: Option<&str>, text: &str| Outcome::Found {
        title: title.map(String::from),
        text: text.into(),
    };
    let read = |body| native_model::outcome(200, body).unwrap();
    assert_eq!(
        read(r#"{"type":"standard","title":"Hund","extract":"Der Hund ist ein Haustier.","k":1}"#),
        found(Some("Hund"), "Der Hund ist ein Haustier.")
    );
    assert_eq!(
        read(r#"{"title":"A \"quoted\" ä name","extract":"Line\nbreak"}"#),
        found(Some("A \"quoted\" ä name"), "Line\nbreak")
    );
    for body in [
        r#"{"title":"No extract"}"#,
        r#"{"extract":"   "}"#,
        r#"{"extract":null,"title":"T"}"#,
        "not json at all",
        "",
    ] {
        assert_eq!(read(body), Outcome::NotFound, "{body}");
    }
}

#[test]
fn the_statuses_say_what_they_mean() {
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
