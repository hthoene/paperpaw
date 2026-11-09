//! The native model is the interface-6 code where they overlap: the target
//! language, the request (service, body, the line that carries the key) and the
//! translation read from an answer.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;
use crate::{AUTO, LANGUAGES, MAX_CHARS, native_model as model, request, target, translation};
use alloc::{format, string::String, vec::Vec};

/// A small deterministic generator, so that failures repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn the_target_language_is_the_interface_6_one() {
    let mut tos: Vec<&str> = alloc::vec![AUTO, "translate.to.", "", "x", "translate.to.xx"];
    tos.extend(LANGUAGES);
    for to in tos {
        for ui in [
            "", "en", "en-GB", "de", "de-AT", "DE", "pt", "pt_BR", "zh", "zh-Hant", "no", "nn",
            "nb", "xx", "sv", "uk", "ja-JP", "tr", "-", "e", "id",
        ] {
            assert_eq!(model::target(to, ui).unwrap(), target(to, ui), "{to} {ui}");
        }
    }
    // A code over the bound is none: the reader's own language is asked for.
    let long = "translate.to.abcdefghijklmnopqrstuvwxyz";
    assert_eq!(model::target(long, "de").unwrap(), "DE");
}

#[test]
fn the_request_is_the_interface_6_request() {
    let mut rng = Rng(9);
    let alphabet: Vec<char> = "ab Z09-_.~ &=+%/\u{e4}\u{4e16}\u{1f600}\n\"'"
        .chars()
        .collect();
    let mut texts: Vec<String> = alloc::vec![
        String::new(),
        "Hello world".into(),
        "Ärger & Öl = 100%".into(),
        "x".repeat(MAX_CHARS),
        "y".repeat(MAX_CHARS + 50),
        "\u{4e16}".repeat(MAX_CHARS + 1),
    ];
    for _ in 0..200 {
        texts.push(
            (0..rng.below(80))
                .map(|_| alphabet[rng.below(alphabet.len())])
                .collect(),
        );
    }
    for key in ["abc:fx", "abc", "", ":fx", "k:fx:"] {
        for text in &texts {
            let old = request(key, text, "DE");
            assert_eq!(model::url(key).unwrap(), old.url());
            assert_eq!(
                Some(model::body(text, "DE").unwrap().as_str()),
                old.body_text()
            );
            let auth = old
                .headers()
                .iter()
                .find(|(name, _)| name == "Authorization")
                .map(|(_, value)| value.as_str());
            assert_eq!(Some(model::authorization(key).unwrap().as_str()), auth);
        }
    }
    assert_eq!(model::host("a:fx"), "api-free.deepl.com");
    assert_eq!(model::host("a"), "api.deepl.com");
    let secret = model::authorization("hunter2").unwrap();
    assert!(!format!("{secret:?}").contains("hunter2"));
}

#[test]
fn the_translation_is_the_interface_6_translation() {
    let bodies = [
        r#"{"translations":[{"detected_source_language":"EN","text":"Hallo \"Welt\""}]}"#,
        r#"{"translations":[{"text":"a\nb ä 😀"}]}"#,
        r#"{"translations":[]}"#,
        r#"{"translations":[{"detected_source_language":"EN"}]}"#,
        r#"{"translations":[{"text":5}]}"#,
        r#"{"translations":[{"text":"first"},{"text":"second"}]}"#,
        r#"{"text":"outside","translations":[{"text":"inside"}]}"#,
        r#"{"translations"#,
        r#"{"translations":"#,
        "{}",
        "",
        "not json",
    ];
    for body in bodies {
        assert_eq!(
            model::translation(body).unwrap(),
            translation(body),
            "{body}"
        );
        for status in [200, 204, 299, 401, 403, 456, 0, 500, 300] {
            let outcome = model::outcome(status, body).unwrap();
            let expected = match status {
                200..=299 => {
                    translation(body).map_or(model::Outcome::Failed, model::Outcome::Translated)
                }
                401 | 403 => model::Outcome::BadKey,
                456 => model::Outcome::Quota,
                0 => model::Outcome::Unreachable,
                _ => model::Outcome::Failed,
            };
            assert_eq!(outcome, expected, "{status} {body}");
        }
    }
}

#[test]
fn an_answer_over_the_bound_is_refused() {
    let body = format!(
        r#"{{"translations":[{{"text":"{}"}}]}}"#,
        "a".repeat(model::MAX_BODY)
    );
    assert_eq!(
        model::translation(&body),
        Err(paperpaw_sdk::native::Error::NoSpace)
    );
    // The longest text of 3,000 characters fits whole.
    let big = "\u{4e16}".repeat(MAX_CHARS);
    let body = format!(r#"{{"translations":[{{"text":"{big}"}}]}}"#);
    assert_eq!(
        model::translation(&body).unwrap().as_deref(),
        Some(big.as_str())
    );
}

#[test]
fn any_body_is_read_without_a_panic() {
    let mut rng = Rng(4);
    let alphabet: Vec<char> = "{}[]\":,\\utextrans lations0\u{e4}\n".chars().collect();
    for _ in 0..3000 {
        let body: String = (0..rng.below(120))
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        let _ = model::translation(&body).unwrap();
        // The same answers as interface 6 where it does not panic.
        if let Ok(old) = std::panic::catch_unwind(|| translation(&body)) {
            assert_eq!(model::translation(&body).unwrap(), old, "{body:?}");
        }
    }
}
