use super::*;
use crate::alloc_refusal_tests::Quiet;
use crate::sync::tests::heap::{armed, fixture};
use alloc::format;
use alloc::string::ToString;
use paperpaw_sdk::MemoryStorage;

const KEY: &str = "abc";

fn request<'a>(method: &'a str, target: &'a str, body: Option<&'a str>, len: u64) -> Request<'a> {
    Request {
        method,
        target,
        body: body.map(|_| "body"),
        body_len: len,
        at: Instant(0),
    }
}

/// One request with a fresh guard: the status, the body told, and what it did.
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
    let len = body.map_or(0, |b| b.len() as u64);
    let (answered, done) = answer(&mut guard, storage, &request(method, target, body, len));
    let words = match answered.body {
        Body::Text(words) => words.to_string(),
        Body::Owned(words) => words,
        Body::Page => String::from("<page>"),
    };
    (answered.status, words, done)
}

/// The words of a body, for a message.
fn told(body: &Body) -> &str {
    match body {
        Body::Text(words) => words,
        Body::Owned(words) => words,
        Body::Page => "<page>",
    }
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
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(card)).0,
        200
    );
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some("back=x")).0,
        422
    );
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=9", Some(card)).0,
        404
    );
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

    let edit = "id=1&front=Hund&back=the+dog";
    assert_eq!(
        ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(edit)).0,
        200
    );
    assert_eq!(cards::page(&storage, 1, 0, 1).0[0].back, "the dog");
    assert_eq!(
        ask(&storage, "POST", "/api/card/delete?k=abc&deck=1&id=2", None).0,
        200
    );
    assert_eq!(
        ask(&storage, "POST", "/api/card/delete?k=abc&deck=1&id=2", None).0,
        404
    );
    let rename = Some("deck=1&name=Words");
    assert_eq!(ask(&storage, "POST", "/api/deck?k=abc", rename).0, 200);
    assert_eq!(cards::decks(&storage).unwrap()[0].name, "Words");
    assert_eq!(
        ask(&storage, "POST", "/api/deck/delete?k=abc&deck=1", None).0,
        200
    );
    assert!(cards::decks(&storage).unwrap().is_empty());
    assert_eq!(ask(&storage, "GET", "/nothing?k=abc", None).0, 404);
    // A method the path does not take is not found.
    assert_eq!(ask(&storage, "GET", "/api/card?k=abc&deck=1", None).0, 404);
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
    let mouse = "front=Maus&back=mouse";
    ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(mouse));
    let stale = format!("id=2&v={cat}&front=Katze&back=the+cat");
    let status = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&stale)).0;
    assert_eq!(status, 404);
    assert_eq!(ask(&storage, "POST", &delete, None).0, 404);
    let (cards, total) = cards::page(&storage, 1, 0, 9);
    assert_eq!((total, cards[1].front.as_str()), (2, "Maus"));
    assert_eq!(cards[1].back, "mouse");

    let fresh = format!("id=1&v={dog}&front=Hund&back=the+dog");
    let status = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&fresh)).0;
    assert_eq!(status, 200);
    let stale = format!("id=1&v={dog}&front=Hund&back=a+dog");
    let (status, body, done) = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&stale));
    assert_eq!((status, done), (409, None), "{body}");
    let delete = format!("/api/card/delete?k=abc&deck=1&id=1&v={dog}");
    assert_eq!(ask(&storage, "POST", &delete, None).0, 409);
    assert_eq!(cards::page(&storage, 1, 0, 1).0[0].back, "the dog");
}

/// Without the key nothing is answered, and five wrong ones lock the page for a
/// minute, the right one too.
#[test]
fn the_key_guards_every_request_and_wrong_ones_lock() {
    let storage = MemoryStorage::new();
    let mut guard = Guard::new(KEY.into());
    let status = |guard: &mut Guard, target: &str, at: u64| {
        let asked = request("GET", target, None, 0);
        let asked = Request {
            at: Instant(at),
            ..asked
        };
        answer(guard, &storage, &asked).0.status
    };
    // No key at all, a browser's icon: turned away, and not counted.
    for _ in 0..9 {
        assert_eq!(status(&mut guard, "/api/decks", 0), 403);
        assert_eq!(status(&mut guard, "/favicon.ico", 0), 404);
    }
    assert_eq!(status(&mut guard, "/api/decks?k=abc", 0), 200);
    for _ in 0..5 {
        assert_eq!(status(&mut guard, "/api/decks?k=nope", 1_000), 403);
    }
    assert_eq!(status(&mut guard, "/api/decks?k=abc", 2_000), 429);
    assert_eq!(status(&mut guard, "/", 2_000), 429);
    // After the minute it is open again.
    assert_eq!(status(&mut guard, "/api/decks?k=abc", 70_000), 200);
}

/// What a client on the network may send is not trusted: texts that are not
/// text, numbers that are not numbers and escapes cut short are refused or
/// read as they stand, never a panic.
#[test]
fn what_is_not_understood_is_refused_cleanly() {
    let storage = MemoryStorage::new();
    ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
    // A key that is not text.
    assert_eq!(ask(&storage, "GET", "/api/decks?k=%ff%fe", None).0, 400);
    // A body that is not text.
    storage
        .write("body", &[b'f', b'r', b'o', b'n', b't', b'=', 0xff, 0xfe])
        .unwrap();
    let mut guard = Guard::new(KEY.into());
    let asked = request("POST", "/api/card?k=abc&deck=1", Some("x"), 8);
    assert_eq!(answer(&mut guard, &storage, &asked).0.status, 400);
    // Numbers that are none, and none at all.
    for target in [
        "/api/cards?k=abc&deck=x",
        "/api/cards?k=abc",
        "/api/card/delete?k=abc&deck=1",
        "/api/deck/delete?k=abc&deck=-1",
    ] {
        let method = if target.contains("delete") {
            "POST"
        } else {
            "GET"
        };
        assert_eq!(ask(&storage, method, target, None).0, 404, "{target}");
    }
    // An escape cut short or not hex stands as written.
    let (status, _, done) = ask(
        &storage,
        "POST",
        "/api/deck?k=abc",
        Some("name=100%25+%zz+%4"),
    );
    assert_eq!(
        (status, done),
        (200, Some(Done::Deck("100% %zz %4".into())))
    );
    // A card without a body is a card without a front.
    assert_eq!(ask(&storage, "POST", "/api/card?k=abc&deck=1", None).0, 422);
    // An import without a body adds nothing.
    let (status, body, done) = ask(&storage, "POST", "/api/import?k=abc&deck=1", None);
    assert_eq!((status, body.as_str(), done), (200, "{\"added\":0}", None));
    // A request for a file of the host's is not a path of the page.
    assert_eq!(
        ask(&storage, "GET", "/../cards/decks.txt?k=abc", None).0,
        404
    );
}

/// What is too long is refused before a byte of it is read: its file is not
/// even there.
#[test]
fn too_much_is_refused_before_it_is_read() {
    let storage = MemoryStorage::new();
    ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
    let mut guard = Guard::new(KEY.into());
    let mut status = |method, target, len| {
        let asked = request(method, target, Some("not there"), len);
        answer(&mut guard, &storage, &asked).0.status
    };
    assert_eq!(status("POST", "/api/card?k=abc&deck=1", MAX_FORM + 1), 413);
    assert_eq!(status("POST", "/api/deck?k=abc", MAX_FORM + 1), 413);
    assert_eq!(
        status("POST", "/api/import?k=abc&deck=1", MAX_IMPORT + 1),
        413
    );
    // At the limit it is read (and its file, here, is not there).
    storage.remove("body").unwrap();
    assert_eq!(status("POST", "/api/card?k=abc&deck=1", MAX_FORM), 404);
    // A form of exactly the limit is taken whole.
    let front = "a".repeat(MAX_FORM as usize - 6);
    let form = format!("front={front}");
    assert_eq!(form.len() as u64, MAX_FORM);
    let (status, _, done) = ask(&storage, "POST", "/api/card?k=abc&deck=1", Some(&form));
    assert_eq!(status, 200);
    assert_eq!(done, Some(Done::Card("a".repeat(cards::MAX_TEXT))));
}

#[test]
fn the_page_has_its_texts_and_limits_where_it_marks_them() {
    let page = assemble("{a:\"/*MAX_IMPORT*/0\"}").unwrap();
    let page = core::str::from_utf8(&page).unwrap();
    assert!(page.contains("{a:\"/*MAX_IMPORT*/0\"}"));
    // A value that holds a mark is not filled again.
    assert_eq!(page.matches("/*MAX_IMPORT*/0").count(), 1);
    assert!(!page.contains(STRINGS_MARK));
    assert!(!page.contains(TEXT_MARK));
    assert!(page.contains(&MAX_IMPORT.to_string()));
    assert!(page.contains(&cards::MAX_TEXT.to_string()));
    // The marks come in the order they are filled in.
    let at = |mark: &str| HTML.find(mark).unwrap();
    assert!(at(STRINGS_MARK) < at(IMPORT_MARK) && at(IMPORT_MARK) < at(TEXT_MARK));
}

/// Every text the page names is in both catalogs, and the page reads only
/// texts that are given.
#[test]
fn every_text_of_the_page_is_given_in_both_languages() {
    for catalog in [
        include_str!("../../i18n/en.ini"),
        include_str!("../../i18n/de.ini"),
    ] {
        for (_, key) in TEXTS {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{key} "))),
                "{key}"
            );
        }
    }
    for part in HTML.split("T.").skip(1) {
        let end = part
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(part.len());
        let name = &part[..end];
        assert!(TEXTS.iter().any(|(given, _)| *given == name), "{name}");
    }
    // The import's texts (en and de) name the most cards a deck holds.
    assert_eq!(cards::MAX_CARDS, 20_000);
    for (text, number) in [
        (include_str!("../../i18n/en.ini"), "20,000"),
        (include_str!("../../i18n/de.ini"), "20.000"),
    ] {
        for key in ["web.import-help", "web.partial", "web.full"] {
            let line = text
                .lines()
                .find(|l| l.starts_with(&format!("vocabulary.{key} ")))
                .unwrap();
            assert!(line.contains(number), "{key}: {line}");
        }
    }
}

#[test]
fn a_text_is_a_json_string_that_is_safe_in_a_script() {
    let mut out = String::new();
    push_json(&mut out, "a\"b\\c<d>\n\u{1}é").unwrap();
    assert_eq!(out, "\"a\\\"b\\\\c\\u003cd>\\u000a\\u0001é\"");
}

/// The answers when the heap refuses each of its allocations in turn: a
/// status of the page or *503*, never a panic, and the decks as they were or
/// as the request made them (whole).
#[test]
fn answers_when_the_heap_refuses_are_clean() {
    let make = || {
        let storage = Quiet::default();
        ask(&storage, "POST", "/api/deck?k=abc", Some("name=Words"));
        ask(
            &storage,
            "POST",
            "/api/card?k=abc&deck=1",
            Some("front=a&back=b"),
        );
        storage
    };
    let requests: [(&str, &str, Option<&str>); 7] = [
        ("GET", "/api/decks?k=abc", None),
        ("GET", "/api/cards?k=abc&deck=1", None),
        ("POST", "/api/deck?k=abc", Some("name=More")),
        ("POST", "/api/card?k=abc&deck=1", Some("front=c&back=d")),
        ("POST", "/api/import?k=abc&deck=1", Some("e\tf\ng\th\n")),
        (
            "POST",
            "/api/card?k=abc&deck=1",
            Some("id=1&front=a&back=changed"),
        ),
        ("POST", "/api/card/delete?k=abc&deck=1&id=1", None),
    ];
    for (method, target, body) in requests {
        let storage = fixture(make);
        // Only the plugin's own allocations are counted and refused: the
        // guard, the body file and the request are made before.
        let storage_len = body.map_or(0, |b| b.len() as u64);
        let prepare = |storage: &dyn Storage| {
            if let Some(body) = body {
                storage.write("body", body.as_bytes()).unwrap();
            }
            Guard::new(KEY.into())
        };
        let asked = request(method, target, body, storage_len);
        let mut guard = prepare(&storage);
        let ((whole, _), count) = armed(usize::MAX, || answer(&mut guard, &storage, &asked));
        assert_eq!(whole.status, 200, "{target}: {}", told(&whole.body));
        assert!(count > 0, "{target} allocates nothing");
        let made = fixture(|| cards::decks(&storage)).map_or(0, |d| d.len());
        for at in 1..=count {
            let storage = fixture(make);
            let mut guard = prepare(&storage);
            let ((refused, _), _) = armed(at, || answer(&mut guard, &storage, &asked));
            assert!(
                matches!(refused.status, 200 | 503),
                "{target}: allocation {at} refused gave {} {}",
                refused.status,
                told(&refused.body)
            );
            // The decks read whole afterwards.
            let decks = fixture(|| cards::decks(&storage));
            // As they were or as the request made them, never half.
            let len = decks.map_or(0, |d| d.len());
            assert!(len == 1 || len == made, "{target} at {at}: {len} decks");
        }
    }
    // The page itself.
    let (whole, count) = armed(usize::MAX, || assemble("{}"));
    assert!(whole.is_ok() && count > 0);
    for at in 1..=count {
        let (refused, _) = armed(at, || assemble("{}"));
        assert!(matches!(refused, Ok(_) | Err(Error::NoMemory)), "{at}");
    }
}
