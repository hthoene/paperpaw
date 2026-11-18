//! The phone's page and its requests, over a fake body store.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::ToString;
use paperpaw_sdk::Instant;

struct Bodies(Vec<(String, Vec<u8>)>);

impl Texts for Bodies {
    fn read(&mut self, body: &str, limit: usize) -> Result<String, Error> {
        let (_, bytes) = self
            .0
            .iter()
            .find(|(name, _)| name == body)
            .ok_or(Error::NotFound)?;
        if bytes.len() > limit {
            return Err(Error::NoSpace);
        }
        String::from_utf8(bytes.clone()).map_err(|_| Error::Utf8)
    }
}

fn ask(
    bodies: &mut Bodies,
    guard: &mut Guard,
    method: &str,
    target: &str,
    body: Option<&str>,
) -> (Answer, Option<String>) {
    let length = body
        .and_then(|name| bodies.0.iter().find(|(n, _)| n == name))
        .map_or(0, |(_, bytes)| bytes.len() as u64);
    let request = Request {
        method,
        target,
        body,
        body_len: length,
        at: Instant(0),
    };
    answer(bodies, guard, &request, 8)
}

fn state(text: &str) -> (Bodies, Guard) {
    (
        Bodies(alloc::vec![(
            ".received/2/1".to_string(),
            text.as_bytes().to_vec()
        )]),
        Guard::new("key".to_string()),
    )
}

#[test]
fn the_page_takes_its_words_and_a_password_gets_no_text() {
    let words = Words {
        send: "Send",
        sent: "Sent",
        failed: "Failed",
        show: "Show",
    };
    let plain = page(&words, "Name \"x\"", "old </script> text", false).unwrap();
    let plain = core::str::from_utf8(&plain).unwrap();
    assert!(plain.contains("send:\"Send\""));
    assert!(plain.contains("title:\"Name \\\"x\\\"\""));
    assert!(plain.contains("text:\"old \\u003c/script> text\""));
    assert!(plain.contains("password:false"));
    assert!(!plain.contains(PLACEHOLDER));
    let secret = page(&words, "Password", "hunter2", true).unwrap();
    let secret = core::str::from_utf8(&secret).unwrap();
    assert!(!secret.contains("hunter2"));
    assert!(secret.contains("text:\"\"") && secret.contains("password:true"));
}

#[test]
fn the_page_reads_only_what_is_given() {
    for part in PAGE.split("S.").skip(1) {
        let end = part
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(part.len());
        assert!(
            [
                "send", "sent", "failed", "show", "title", "text", "password"
            ]
            .contains(&&part[..end]),
            "{}",
            &part[..end]
        );
    }
}

#[test]
fn the_text_comes_back_with_the_key_and_not_without() {
    let (mut bodies, mut guard) = state("hello");
    let (page, text) = ask(&mut bodies, &mut guard, "GET", "/?k=key", None);
    assert_eq!(
        (page.status, page.body, text),
        (200, Body::Data(PAGE_FILE), None)
    );
    let (refused, text) = ask(
        &mut bodies,
        &mut guard,
        "POST",
        "/send",
        Some(".received/2/1"),
    );
    assert_eq!((refused.status, text), (403, None));
    let (sent, text) = ask(
        &mut bodies,
        &mut guard,
        "POST",
        "/send?k=key",
        Some(".received/2/1"),
    );
    assert_eq!((sent.status, text.as_deref()), (200, Some("hello")));
    // An empty send is an empty text.
    let (sent, text) = ask(&mut bodies, &mut guard, "POST", "/send?k=key", None);
    assert_eq!((sent.status, text.as_deref()), (200, Some("")));
}

#[test]
fn a_text_too_long_or_not_text_is_refused_and_nothing_comes_back() {
    let (mut bodies, mut guard) = state("too long for eight");
    let (refused, text) = ask(
        &mut bodies,
        &mut guard,
        "POST",
        "/send?k=key",
        Some(".received/2/1"),
    );
    assert_eq!((refused.status, text), (413, None));
    bodies
        .0
        .push((".received/2/2".to_string(), alloc::vec![0xff, 0xfe]));
    let (refused, text) = ask(
        &mut bodies,
        &mut guard,
        "POST",
        "/send?k=key",
        Some(".received/2/2"),
    );
    assert_eq!((refused.status, text), (400, None));
}

#[test]
fn other_places_are_not_found_and_the_icon_is_no_guess_at_the_key() {
    let (mut bodies, mut guard) = state("x");
    assert_eq!(
        ask(&mut bodies, &mut guard, "GET", "/api/list?k=key", None)
            .0
            .status,
        404
    );
    assert_eq!(
        ask(&mut bodies, &mut guard, "GET", "/favicon.ico", None)
            .0
            .status,
        404
    );
    for _ in 0..paperpaw_sdk::served::WRONG_KEYS {
        ask(&mut bodies, &mut guard, "GET", "/?k=no", None);
    }
    assert_eq!(
        ask(&mut bodies, &mut guard, "GET", "/?k=key", None)
            .0
            .status,
        429
    );
}
