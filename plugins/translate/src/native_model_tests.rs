//! The model's answers, pinned: the target language, the request (service,
//! body, the line that carries the key) and the translation read from an
//! answer are what the interface-6 plugin gave (the values below were taken
//! from it before it was removed).
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;
use crate::declarations::{AUTO, LANGUAGES};
use crate::native_model::{self as model, MAX_CHARS, Outcome};
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::web::percent_decode;
use proptest::prelude::*;

#[test]
fn the_target_language_is_what_interface_6_chose() {
    for (to, ui, expected) in [
        (AUTO, "de", "DE"),
        (AUTO, "en", "EN-GB"),
        (AUTO, "en-GB", "EN-GB"),
        (AUTO, "pt_BR", "PT-PT"),
        (AUTO, "zh", "ZH-HANS"),
        (AUTO, "zh-Hant", "ZH-HANS"),
        (AUTO, "no", "NB"),
        (AUTO, "nn", "NB"),
        (AUTO, "xx", "EN-GB"),
        (AUTO, "", "EN-GB"),
        (AUTO, "DE", "DE"),
        (AUTO, "de-AT", "DE"),
        (AUTO, "ja-JP", "JA"),
        (AUTO, "id", "ID"),
        ("translate.to.fr", "de", "FR"),
        ("translate.to.pt-br", "en", "PT-BR"),
        ("translate.to.en-us", "de", "EN-US"),
        ("", "de", "DE"),
        ("x", "de", "DE"),
        ("translate.to.xx", "de", "XX"),
    ] {
        assert_eq!(model::target(to, ui).unwrap(), expected, "{to} {ui}");
    }
    // A code over the bound is none: the reader's own language is asked for.
    let long = "translate.to.abcdefghijklmnopqrstuvwxyz";
    assert_eq!(model::target(long, "de").unwrap(), "DE");
}

#[test]
fn the_request_is_what_interface_6_sent() {
    let text = "Hello & Ärger = 100%\n世界";
    let body = "text=Hello+%26+%C3%84rger+%3D+100%25%0A%E4%B8%96%E7%95%8C&target_lang=DE";
    for (key, url) in [
        ("abc:fx", "https://api-free.deepl.com/v2/translate"),
        ("abc", "https://api.deepl.com/v2/translate"),
        ("", "https://api.deepl.com/v2/translate"),
        (":fx", "https://api-free.deepl.com/v2/translate"),
        ("k:fx:", "https://api.deepl.com/v2/translate"),
    ] {
        assert_eq!(model::url(key).unwrap(), url, "{key:?}");
        assert_eq!(model::body(text, "DE").unwrap(), body);
        assert_eq!(
            model::authorization(key).unwrap().as_str(),
            format!("DeepL-Auth-Key {key}")
        );
    }
    assert_eq!(model::host("a:fx"), "api-free.deepl.com");
    assert_eq!(model::host("a"), "api.deepl.com");
    let secret = model::authorization("hunter2").unwrap();
    assert!(!format!("{secret:?}").contains("hunter2"));
}

#[test]
fn a_text_is_cut_to_its_limit_in_characters() {
    let long = "\u{4e16}".repeat(MAX_CHARS + 50);
    let sent = model::body(&long, "DE").unwrap();
    let field = sent
        .strip_prefix("text=")
        .and_then(|rest| rest.strip_suffix("&target_lang=DE"))
        .unwrap();
    assert_eq!(
        percent_decode(&field.replace('+', " ")),
        "\u{4e16}".repeat(MAX_CHARS)
    );
}

#[test]
fn the_translation_is_what_interface_6_read() {
    for (body, expected) in [
        (
            r#"{"translations":[{"detected_source_language":"EN","text":"Hallo \"Welt\""}]}"#,
            Some("Hallo \"Welt\""),
        ),
        (
            r#"{"translations":[{"text":"a\nb ä 😀"}]}"#,
            Some("a\nb ä 😀"),
        ),
        (r#"{"translations":[]}"#, None),
        (
            r#"{"translations":[{"detected_source_language":"EN"}]}"#,
            None,
        ),
        (r#"{"translations":[{"text":5}]}"#, None),
        (
            r#"{"translations":[{"text":"first"},{"text":"second"}]}"#,
            Some("first"),
        ),
        (
            r#"{"text":"outside","translations":[{"text":"inside"}]}"#,
            Some("inside"),
        ),
        (r#"{"translations"#, None),
        (r#"{"translations":"#, None),
        ("{}", None),
        ("", None),
        ("not json", None),
    ] {
        let read = model::translation(body).unwrap();
        assert_eq!(read.as_deref(), expected, "{body}");
        for status in [200, 204, 299, 401, 403, 456, 0, 500, 300] {
            let expected = match status {
                200..=299 => read.clone().map_or(Outcome::Failed, Outcome::Translated),
                401 | 403 => Outcome::BadKey,
                456 => Outcome::Quota,
                0 => Outcome::Unreachable,
                _ => Outcome::Failed,
            };
            assert_eq!(
                model::outcome(status, body).unwrap(),
                expected,
                "{status} {body}"
            );
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
fn the_languages_are_the_32_of_deepl_in_order() {
    assert_eq!(LANGUAGES.len(), 32);
    assert_eq!(LANGUAGES[0], "translate.to.ar");
    assert_eq!(LANGUAGES[31], "translate.to.zh-hans");
    assert!(LANGUAGES.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(LANGUAGES.iter().all(|l| l.starts_with("translate.to.")));
    assert_eq!(AUTO, "translate.to.auto");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn any_body_reads_without_panic(body in ".{0,300}") {
        let _ = model::translation(&body).unwrap();
        let wrapped = format!("{{\"translations\":[{{\"text\":{body}}}]}}");
        let _ = model::translation(&wrapped).unwrap();
    }

    #[test]
    fn every_choice_gives_a_target_deepl_knows(
        to in prop_oneof![Just(String::from(AUTO)), prop::sample::select(LANGUAGES.to_vec()).prop_map(String::from)],
        ui in ".{0,10}",
    ) {
        let code = model::target(&to, &ui).unwrap();
        prop_assert!(
            LANGUAGES
                .iter()
                .any(|l| l.strip_prefix("translate.to.").map(str::to_uppercase) == Some(code.clone())),
            "{} for {:?} {:?}", code, to, ui
        );
    }

    #[test]
    fn any_text_goes_whole_up_to_its_limit(text in ".{0,3500}") {
        let body = model::body(&text, "DE").unwrap();
        let (field, rest) = body.split_once("&target_lang=").unwrap();
        prop_assert_eq!(rest, "DE");
        let sent = field.strip_prefix("text=").unwrap();
        prop_assert!(!sent.contains(['&', '=', ' ']));
        let decoded = percent_decode(&sent.replace('+', " "));
        let expected: String = text.chars().take(MAX_CHARS).collect();
        prop_assert_eq!(decoded, expected);
    }
}

/// Bodies of random pieces of JSON never panic the reader.
#[test]
fn any_random_body_is_read_without_a_panic() {
    let alphabet: Vec<char> = "{}[]\":,\\utextrans lations0\u{e4}\n".chars().collect();
    let mut state = 4u64;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) % n as u64) as usize
    };
    for _ in 0..3000 {
        let body: String = (0..next(120))
            .map(|_| alphabet[next(alphabet.len())])
            .collect();
        let _ = model::translation(&body).unwrap();
    }
}
