//! The phone page's requests over a memory storage: what interface 6's tests
//! asked, on the native answers; and the heap refusing.
use super::*;
use crate::alloc_refusal_tests::Quiet;
use crate::sync::tests::heap::{armed, fixture};
use alloc::format;
use paperpaw_sdk::MemoryStorage;

const KEY: &str = "abc";

fn ask(
    storage: &dyn Storage,
    method: &str,
    target: &str,
    body: Option<&str>,
) -> (u16, String, Option<Done>) {
    if let Some(body) = body {
        storage.write("body", body.as_bytes()).unwrap();
    }
    let mut guard = Guard::new(KEY.into());
    let request = Request {
        method,
        target,
        body: body.map(|_| "body"),
        at: Instant(0),
    };
    let (answer, done) = answer(storage, &mut guard, &request);
    let body = match answer.body {
        Body::Text(words) => words.into(),
        Body::Owned(words) => words,
        Body::Page => "<page>".into(),
    };
    (answer.status, body, done)
}

#[test]
fn decks_and_cards_are_made_on_the_phone() {
    let storage = MemoryStorage::new();
    assert_eq!(ask(&storage, "GET", "/?k=abc", None).0, 200);
    assert_eq!(ask(&storage, "GET", "/?k=wrong", None).0, 403);
    assert_eq!(ask(&storage, "GET", "/api/decks", None).0, 403);
    let (status, body, done) = ask(
        &storage,
        "POST",
        "/api/deck?k=abc",
        Some("name=W%C3%B6rter"),
    );
    assert_eq!((status, body.as_str()), (200, "{\"id\":1}"));
    assert_eq!(done, Some(Done::Deck("Wörter".into())));
    let card = "front=Hund&back=dog&example=Der+Hund+bellt.";
    let post = |target: &str, form: &str| ask(&storage, "POST", target, Some(form)).0;
    assert_eq!(post("/api/card?k=abc&deck=1", card), 200);
    assert_eq!(post("/api/card?k=abc&deck=1", "back=x"), 422);
    assert_eq!(post("/api/card?k=abc&deck=9", card), 404);
    let import = "Katze\tcat\nMaus\tmouse\tDie Maus.\n";
    let (status, body, done) = ask(&storage, "POST", "/api/import?k=abc&deck=1", Some(import));
    assert_eq!(
        (status, body.as_str(), done),
        (200, "{\"added\":2}", Some(Done::Imported(2)))
    );
    let (_, decks, _) = ask(&storage, "GET", "/api/decks?k=abc", None);
    assert_eq!(
        decks,
        "{\"decks\":[{\"id\":1,\"name\":\"Wörter\",\"cards\":3}]}"
    );
    let (_, list, _) = ask(&storage, "GET", "/api/cards?k=abc&deck=1&from=2", None);
    assert!(list.starts_with("{\"cards\":[{\"id\":3,\"v\":"), "{list}");
    assert!(list.ends_with("],\"total\":3}"), "{list}");

    assert_eq!(
        post("/api/card?k=abc&deck=1", "id=1&front=Hund&back=the+dog"),
        200
    );
    assert_eq!(cards::page(&storage, 1, 0, 1).0[0].back, "the dog");
    let delete = |id: u32| {
        ask(
            &storage,
            "POST",
            &format!("/api/card/delete?k=abc&deck=1&id={id}"),
            None,
        )
        .0
    };
    assert_eq!((delete(2), delete(2)), (200, 404));
    assert_eq!(post("/api/deck?k=abc", "deck=1&name=Words"), 200);
    assert_eq!(cards::decks(&storage).unwrap()[0].name, "Words");
    let (status, body, done) = ask(&storage, "POST", "/api/deck/delete?k=abc&deck=1", None);
    assert_eq!(
        (status, body.as_str(), done),
        (200, "{}", Some(Done::Deleted))
    );
    assert!(cards::decks(&storage).unwrap().is_empty());
    assert_eq!(ask(&storage, "GET", "/nothing?k=abc", None).0, 404);
}

/// Two tabs open: one deletes the newest card and the other adds one. What the
/// first then sends for the card it deleted finds nothing, and a change from
/// texts it saw before another tab changed them is refused: neither card is
/// touched.
#[test]
fn a_stale_tab_cannot_change_a_card_it_never_saw() {
    let storage = MemoryStorage::new();
    ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
    for card in ["front=Hund&back=dog", "front=Katze&back=cat"] {
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(card));
    }
    let (_, list, _) = ask(&storage, "GET", "/api/cards?k=abc&deck=1", None);
    let version = |n: usize| cards::page(&storage, 1, n, 1).0[0].version();
    let (dog, cat) = (version(0), version(1));
    assert!(list.contains(&format!("\"id\":2,\"v\":{cat},")), "{list}");

    let delete = format!("/api/card/delete?k=abc&deck=1&id=2&v={cat}");
    assert_eq!(ask(&storage, "POST", &delete, None).0, 200);
    ask(
        &storage,
        "POST",
        "/api/card?k=abc&deck=1",
        Some("front=Maus&back=mouse"),
    );
    let stale = format!("id=2&v={cat}&front=Katze&back=the+cat");
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&stale)).0,
        404
    );
    assert_eq!(ask(&storage, "POST", &delete, None).0, 404);
    let (kept, total) = cards::page(&storage, 1, 0, 9);
    assert_eq!(
        (total, kept[1].front.as_str(), kept[1].back.as_str()),
        (2, "Maus", "mouse")
    );

    let fresh = format!("id=1&v={dog}&front=Hund&back=the+dog");
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&fresh)).0,
        200
    );
    let stale = format!("id=1&v={dog}&front=Hund&back=a+dog");
    let (status, body, done) = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&stale));
    assert_eq!((status, done), (409, None), "{body}");
    let delete = format!("/api/card/delete?k=abc&deck=1&id=1&v={dog}");
    assert_eq!(ask(&storage, "POST", &delete, None).0, 409);
    assert_eq!(cards::page(&storage, 1, 0, 1).0[0].back, "the dog");
}

#[test]
fn wrong_keys_lock_the_page_for_a_while() {
    let storage = MemoryStorage::new();
    let mut guard = Guard::new(KEY.into());
    let mut at = |target: &str, millis: u64| {
        let request = Request {
            method: "GET",
            target,
            body: None,
            at: Instant(millis),
        };
        answer(&storage, &mut guard, &request).0.status
    };
    for _ in 0..5 {
        assert_eq!(at("/?k=no", 0), 403);
    }
    // Locked: not even the right key.
    assert_eq!(at("/?k=abc", 1_000), 429);
    assert_eq!(at("/?k=abc", 10 * 60_000), 200);
}

/// A form that is too long counts as none; an import over the cap is refused;
/// a request that came without a body into an import adds nothing.
#[test]
fn bodies_over_the_limits_are_not_taken() {
    let storage = MemoryStorage::new();
    ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
    let long = format!("front={}&back=x", "a".repeat(MAX_FORM));
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&long)).0,
        422
    );
    let big = alloc::vec![b'a'; MAX_IMPORT as usize + 1];
    storage.write("body", &big).unwrap();
    let request = Request {
        method: "POST",
        target: "/api/import?k=abc&deck=1",
        body: Some("body"),
        at: Instant(0),
    };
    let (answer, done) = answer(&storage, &mut Guard::new(KEY.into()), &request);
    assert_eq!((answer.status, done), (413, None));
    let (status, body, _) = ask(&storage, "POST", "/api/import?k=abc&deck=1", None);
    assert_eq!((status, body.as_str()), (200, "{\"added\":0}"));
}

/// The heap refusing each allocation in turn: an answer is made or a 503
/// (or the reader's own refusal) sent, never a panic, and a request that
/// changes nothing leaves the storage as it was.
#[test]
fn when_the_heap_refuses_the_lists_are_an_answer_not_a_panic() {
    let storage = Quiet(MemoryStorage::new());
    fixture(|| {
        ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
        let card = "front=Hund&back=dog&example=Der+Hund+bellt.";
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(card));
    });
    for target in ["/api/decks?k=abc", "/api/cards?k=abc&deck=1"] {
        let request = Request {
            method: "GET",
            target,
            body: None,
            at: Instant(0),
        };
        // The guard is made outside: it is the test's, not the plugin's.
        let mut guard = fixture(|| Guard::new(KEY.into()));
        let ((reply, _), count) = armed(usize::MAX, || answer(&storage, &mut guard, &request));
        assert_eq!(reply.status, 200, "{target}");
        assert!(
            count > 0,
            "{target} allocates nothing: the sweep proves nothing"
        );
        for at in 1..=count {
            let ((reply, done), _) = armed(at, || answer(&storage, &mut guard, &request));
            let status = reply.status;
            assert!(
                matches!(status, 200 | 500 | 503),
                "{target} at {at}: {status}"
            );
            assert_eq!(done, None);
        }
    }
}
