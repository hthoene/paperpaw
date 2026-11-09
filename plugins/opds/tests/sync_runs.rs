//! Syncing catalogs with the real kernel, the server's answers canned:
//! books land in each catalog's folder and are remembered, pages are
//! followed without looping (a next link to itself, cycles, endless
//! chains), the limit per sync holds, failures are counted and retried
//! later, unreachable catalogs do not stop the others, hostile titles and
//! links stay inside the folder, WebDAV folders are listed with
//! `PROPFIND`, and passwords go with the requests but never into the
//! list in clear.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "harness/mod.rs"]
mod common;

use common::{CATALOGS, DIR, Harness, Shared, books_feed, catalogs};
use paperpaw_core::HttpResponse;
use paperpaw_plugin_opds::{OPEN, SYNC};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::secrets::{FILE, Secrets};
use paperpaw_sdk::{MemoryClass, Storage};

const SHELF: &str = "https://shelf.example/opds/shelf";

fn one_catalog() -> Shared {
    Shared::with(&[(
        CATALOGS,
        &catalogs(&[["Shelf", SHELF, "", "", "Synced", "sync", ""]]),
    )])
}

/// The ids the sync remembers for `url`.
fn remembered(storage: &Shared, url: &str) -> Vec<String> {
    let path = format!("{DIR}/synced/{}.txt", paperpaw_sdk::digest::md5::of(url));
    storage
        .text(&path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// The files outside PaperPaw's own folder.
fn books(storage: &Shared) -> Vec<String> {
    storage
        .files("")
        .into_iter()
        .filter(|p| !p.starts_with(".paperpaw/"))
        .collect()
}

#[test]
fn marked_catalogs_are_synced_into_their_folder_and_remembered() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let feed = h.request();
    assert_eq!(feed.url, SHELF);
    assert_eq!(feed.method, "GET");
    let page = books_feed(
        "Shelf",
        None,
        &[
            ("urn:a", "Alpha", "/books/a.epub"),
            ("urn:b", "Beta", "b.epub"),
        ],
    );
    h.answer(&feed, 200, page.as_bytes());
    let book = h.request();
    // Relative links are resolved against the page they came in.
    assert_eq!(book.url, "https://shelf.example/books/a.epub");
    assert!(book.save_to.is_some());
    h.answer(&book, 200, b"PK alpha");
    let book = h.request();
    assert_eq!(book.url, "https://shelf.example/opds/b.epub");
    h.answer(&book, 200, b"PK beta");
    assert!(h.requests().is_empty(), "the sync is done");
    assert_eq!(books(&h.storage), ["Synced/Alpha.epub", "Synced/Beta.epub"]);
    assert_eq!(h.storage.0.get("Synced/Beta.epub").unwrap(), b"PK beta");
    assert_eq!(remembered(&h.storage, SHELF), ["urn:a", "urn:b"]);
    // What was loading is not left behind.
    let leftovers: Vec<String> = h
        .storage
        .files(DIR)
        .into_iter()
        .filter(|p| p.ends_with(".xml") || p.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn books_synced_before_are_not_fetched_again_even_when_deleted() {
    let mut h = Harness::new(one_catalog());
    let page = books_feed(
        "Shelf",
        None,
        &[("urn:a", "Alpha", "a.epub"), ("urn:b", "Beta", "b.epub")],
    );
    h.run(SYNC);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    for _ in 0..2 {
        let book = h.request();
        h.answer(&book, 200, b"PK");
    }
    // The reader deleted one; the next sync leaves it so.
    h.storage.remove("Synced/Alpha.epub").unwrap();
    h.run(SYNC);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    assert!(h.requests().is_empty());
    assert_eq!(books(&h.storage), ["Synced/Beta.epub"]);
}

#[test]
fn a_book_already_in_the_folder_is_skipped() {
    let storage = one_catalog();
    storage.write("Synced/Alpha.epub", b"mine").unwrap();
    let mut h = Harness::new(storage);
    h.run(SYNC);
    let feed = h.request();
    let page = books_feed(
        "Shelf",
        None,
        &[("urn:a", "Alpha", "a.epub"), ("urn:b", "Beta", "b.epub")],
    );
    h.answer(&feed, 200, page.as_bytes());
    let book = h.request();
    assert!(book.url.ends_with("/b.epub"), "{}", book.url);
    h.answer(&book, 200, b"PK");
    assert!(h.requests().is_empty());
    assert_eq!(h.storage.0.get("Synced/Alpha.epub").unwrap(), b"mine");
}

#[test]
fn a_next_link_to_the_same_page_ends_the_sync() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let feed = h.request();
    // The same page, written relatively.
    let page = books_feed("Shelf", Some("/opds/shelf"), &[]);
    h.answer(&feed, 200, page.as_bytes());
    assert!(
        h.requests().is_empty(),
        "no second request for the same page"
    );
    // The sync is over: a new one may start.
    h.run(SYNC);
    assert_eq!(h.request().url, SHELF);
}

#[test]
fn a_cycle_of_pages_is_read_once() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let first = h.request();
    h.answer(
        &first,
        200,
        books_feed("Shelf", Some("?page=2"), &[]).as_bytes(),
    );
    let second = h.request();
    assert_eq!(second.url, format!("{SHELF}?page=2"));
    // Back to the first page.
    h.answer(
        &second,
        200,
        books_feed("Shelf", Some(SHELF), &[]).as_bytes(),
    );
    assert!(h.requests().is_empty());
}

#[test]
fn an_endless_chain_of_pages_stops_after_fifty() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let mut pages = 0;
    let mut calls = h.requests();
    while let Some(call) = calls.pop() {
        pages += 1;
        assert!(pages <= 200, "the sync does not end");
        let next = format!("?page={}", pages + 1);
        h.answer(&call, 200, books_feed("Shelf", Some(&next), &[]).as_bytes());
        calls = h.requests();
    }
    assert_eq!(pages, 50);
}

#[test]
fn at_most_the_limit_of_books_comes_with_one_sync() {
    let mut h = Harness::build(
        one_catalog(),
        MemoryClass::Small,
        "[opds]\nsync-limit = 10\n",
    );
    let entries: Vec<(String, String, String)> = (0..15)
        .map(|n| (format!("urn:{n}"), format!("Book {n}"), format!("{n}.epub")))
        .collect();
    let entries: Vec<(&str, &str, &str)> = entries
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
        .collect();
    let page = books_feed("Shelf", None, &entries);
    let sync = |h: &mut Harness| {
        h.run(SYNC);
        let feed = h.request();
        h.answer(&feed, 200, page.as_bytes());
        let mut loaded = 0;
        while let Some(book) = h.requests().pop() {
            loaded += 1;
            h.answer(&book, 200, b"PK");
        }
        loaded
    };
    assert_eq!(sync(&mut h), 10);
    assert_eq!(sync(&mut h), 5, "the rest with the next sync");
    assert_eq!(books(&h.storage).len(), 15);
}

#[test]
fn failed_downloads_are_counted_and_tried_again_next_time() {
    let mut h = Harness::new(one_catalog());
    let page = books_feed(
        "Shelf",
        None,
        &[
            ("urn:a", "Alpha", "a.epub"),
            ("urn:b", "Beta", "b.epub"),
            ("urn:c", "Gamma", "c.epub"),
        ],
    );
    h.run(SYNC);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    let a = h.request();
    h.answer(&a, 404, b"gone");
    let b = h.request();
    h.answer(&b, 200, b"PK");
    let c = h.request();
    h.answer(&c, 0, b"timed out");
    assert!(h.requests().is_empty());
    assert_eq!(books(&h.storage), ["Synced/Beta.epub"]);
    assert_eq!(remembered(&h.storage, SHELF), ["urn:b"]);
    // The list of catalogs tells how it went.
    h.run(OPEN);
    let shown = h.said();
    assert!(shown.contains("1 new book"), "{shown}");
    assert!(shown.contains("2 could not be downloaded"), "{shown}");
    // The next sync tries the two again.
    h.run(SYNC);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    let mut urls = Vec::new();
    while let Some(book) = h.requests().pop() {
        urls.push(book.url.clone());
        h.answer(&book, 200, b"PK");
    }
    assert_eq!(
        urls,
        [
            "https://shelf.example/opds/a.epub",
            "https://shelf.example/opds/c.epub"
        ]
    );
}

/// A page the server answers with instead of the book (a sign-in, an
/// error) is not kept as one, nor remembered: it is tried again.
#[test]
fn an_answer_that_is_no_book_is_not_kept() {
    let mut h = Harness::new(one_catalog());
    let page = books_feed("Shelf", None, &[("urn:a", "Alpha", "a.epub")]);
    h.run(SYNC);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    let book = h.request();
    h.answer(
        &book,
        200,
        b"<!DOCTYPE html><html><body>Sign in</body></html>",
    );
    assert!(h.requests().is_empty());
    assert!(books(&h.storage).is_empty(), "{:?}", books(&h.storage));
    assert!(remembered(&h.storage, SHELF).is_empty());
    h.run(SYNC);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    let book = h.request();
    h.answer(&book, 200, b"PK alpha");
    assert_eq!(books(&h.storage), ["Synced/Alpha.epub"]);
}

#[test]
fn an_unreachable_catalog_does_not_stop_the_others() {
    let storage = Shared::with(&[(
        CATALOGS,
        &catalogs(&[
            [
                "Down",
                "https://down.example/opds",
                "",
                "",
                "Down",
                "sync",
                "",
            ],
            [
                "Not marked",
                "https://other.example/opds",
                "",
                "",
                "",
                "",
                "",
            ],
            ["Shelf", SHELF, "", "", "Synced", "sync", ""],
        ]),
    )]);
    let mut h = Harness::new(storage);
    h.run(SYNC);
    let down = h.request();
    assert_eq!(down.url, "https://down.example/opds");
    h.answer(&down, 500, b"Internal Server Error");
    let shelf = h.request();
    assert_eq!(shelf.url, SHELF, "the unmarked catalog is left out");
    h.answer(
        &shelf,
        200,
        books_feed("Shelf", None, &[("urn:a", "Alpha", "a.epub")]).as_bytes(),
    );
    let book = h.request();
    h.answer(&book, 200, b"PK");
    assert!(h.requests().is_empty());
    h.run(OPEN);
    let shown = h.said();
    assert!(shown.contains("1 catalog could not be reached"), "{shown}");
}

#[test]
fn a_catalog_of_lists_only_is_reported_not_followed() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let feed = h.request();
    let lists = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Start</title>
<entry><title>Popular</title><link rel="subsection" type="application/atom+xml" href="/popular"/></entry>
</feed>"#;
    h.answer(&feed, 200, lists.as_bytes());
    assert!(h.requests().is_empty(), "sync follows no further lists");
    h.run(OPEN);
    assert!(h.said().contains("lists no books itself"));
}

#[test]
fn hostile_titles_and_links_stay_inside_the_sync_folder() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let feed = h.request();
    let long = "x".repeat(500);
    let entries = [
        ("urn:1", "../../../etc/passwd", "../../../../one.epub"),
        ("urn:2", "..", "/%2e%2e/%2e%2e/two.epub"),
        ("urn:3", "", "%2F..%2F..%2Fthree.epub"),
        ("urn:4", "a/b\\c:d*e?f&quot;g&lt;h&gt;i|j", "four.epub"),
        ("urn:5", ".hidden", "five.epub"),
        ("urn:6", long.as_str(), "six.epub"),
        ("urn:7", "&#x0;nul&#9;tab", "seven.epub"),
        ("urn:8", "Script", "javascript:alert(1)"),
    ];
    h.answer(&feed, 200, books_feed("Shelf", None, &entries).as_bytes());
    let mut asked = Vec::new();
    while let Some(book) = h.requests().pop() {
        asked.push(book.url.clone());
        // Only addresses the shell can fetch get an answer.
        let status = if book.url.starts_with("https://") {
            200
        } else {
            0
        };
        h.answer(&book, status, b"PK");
    }
    assert!(
        asked
            .iter()
            .all(|u| u.starts_with("https://shelf.example/") || u.starts_with("javascript:")),
        "{asked:?}"
    );
    let saved = books(&h.storage);
    assert_eq!(saved.len(), 7, "{saved:?}");
    for path in &saved {
        let name = path.strip_prefix("Synced/").expect(path);
        assert!(!name.contains('/') && !name.contains('\\'), "{path}");
        assert!(!name.starts_with('.'), "{path}");
        assert!(name.ends_with(".epub"), "{path}");
        assert!(!name.chars().any(char::is_control), "{path:?}");
        assert!(
            !name.contains([':', '*', '?', '"', '<', '>', '|']),
            "{path}"
        );
        assert!(name.chars().count() <= 130, "{path}");
    }
}

#[test]
fn answers_to_a_stopped_sync_are_ignored() {
    let mut h = Harness::new(one_catalog());
    h.run(OPEN);
    // *Sync catalogs* is the first row while a catalog is marked.
    assert!(h.shows("Sync catalogs"));
    h.run(actions::CONFIRM);
    let old = h.request();
    assert!(h.shows("Stop syncing"));
    h.run(actions::CONFIRM);
    assert!(h.shows("Sync catalogs"));
    // The old run's answer comes late: nothing follows from it.
    let page = books_feed("Shelf", None, &[("urn:a", "Alpha", "a.epub")]);
    h.answer(&old, 200, page.as_bytes());
    assert!(h.requests().is_empty());
    // A new run works as ever.
    h.run(actions::CONFIRM);
    let feed = h.request();
    h.answer(&feed, 200, page.as_bytes());
    let book = h.request();
    h.answer(&book, 200, b"PK");
    assert_eq!(books(&h.storage), ["Synced/Alpha.epub"]);
}

#[test]
fn webdav_folders_are_listed_with_propfind_and_synced() {
    let url = "https://cloud.example/remote.php/dav/files/me/Books/";
    let storage = Shared::with(&[(
        CATALOGS,
        &catalogs(&[["Cloud", url, "", "", "Cloud", "sync", "webdav"]]),
    )]);
    let mut h = Harness::new(storage);
    h.run(SYNC);
    let list = h.request();
    assert_eq!(list.method, "PROPFIND");
    assert!(
        list.headers
            .contains(&(String::from("Depth"), String::from("1")))
    );
    let body = String::from_utf8(list.body.clone().unwrap()).unwrap();
    assert!(body.contains("propfind"), "{body}");
    let listing = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">
<d:response><d:href>/remote.php/dav/files/me/Books/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/Die%20Verwandlung.epub</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontenttype>application/epub+zip</d:getcontenttype></d:prop></d:propstat></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/..%2F..%2Fescape.epub</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontenttype>application/epub+zip</d:getcontenttype></d:prop></d:propstat></d:response>
</d:multistatus>"#;
    h.answer(&list, 207, listing.as_bytes());
    let mut asked = Vec::new();
    while let Some(book) = h.requests().pop() {
        asked.push(book.url.clone());
        h.answer(&book, 200, b"PK");
    }
    assert!(
        asked.contains(&format!("{url}Die%20Verwandlung.epub")),
        "{asked:?}"
    );
    let saved = books(&h.storage);
    assert!(
        saved.contains(&String::from("Cloud/Die Verwandlung.epub")),
        "{saved:?}"
    );
    assert!(saved.iter().all(|p| p.starts_with("Cloud/")), "{saved:?}");
}

#[test]
fn passwords_of_old_lists_move_to_the_secrets_and_go_with_requests() {
    // Version 1 lists of earlier releases kept the password in clear.
    let storage = Shared::with(&[(
        CATALOGS,
        &catalogs(&[["Calibre", SHELF, "reader", "s3cr3t-pw", "", "sync", ""]]),
    )]);
    let h = Harness::new(storage.clone());
    // Kept with the other secrets, veiled; the list lets go of it only
    // once they hold it, at a later start.
    let id = format!("opds.{}", &paperpaw_sdk::digest::md5::of(SHELF)[..16]);
    assert_eq!(h.kernel.secrets().get(&id), Some("s3cr3t-pw"));
    assert!(h.storage.text(CATALOGS).unwrap().contains("s3cr3t-pw"));
    drop(h);
    // The secrets were not saved (a full card): the list still has it.
    storage.0.remove(FILE).unwrap();
    let h = Harness::new(storage.clone());
    assert!(h.storage.text(CATALOGS).unwrap().contains("s3cr3t-pw"));
    assert_eq!(h.kernel.secrets().get(&id), Some("s3cr3t-pw"));
    drop(h);
    let mut h = Harness::new(storage);
    let list = h.storage.text(CATALOGS).unwrap();
    assert!(!list.contains("s3cr3t-pw"), "{list}");
    assert!(list.contains("reader"), "{list}");
    assert_eq!(h.kernel.secrets().get(&id), Some("s3cr3t-pw"));
    let file = h.storage.text(FILE).unwrap();
    assert!(!file.contains("s3cr3t-pw"), "{file}");
    assert_eq!(Secrets::decode(&file).get(&id), Some("s3cr3t-pw"));
    // No file holds it in clear.
    for path in h.storage.files("") {
        let data = h.storage.0.get(&path).unwrap();
        assert!(
            !data.windows(9).any(|w| w == b"s3cr3t-pw"),
            "{path} holds the password"
        );
    }
    // And it goes with the sync's requests.
    h.run(SYNC);
    let feed = h.request();
    let auth = paperpaw_sdk::web::basic_auth("reader", "s3cr3t-pw");
    assert!(
        feed.headers
            .contains(&(String::from("Authorization"), auth.clone())),
        "{:?}",
        feed.headers
    );
    h.answer(
        &feed,
        200,
        books_feed("Shelf", None, &[("urn:a", "Alpha", "a.epub")]).as_bytes(),
    );
    let book = h.request();
    assert!(
        book.headers
            .contains(&(String::from("Authorization"), auth))
    );
}

#[test]
fn a_redirected_feed_resolves_links_against_where_it_came_from() {
    let mut h = Harness::new(one_catalog());
    h.run(SYNC);
    let feed = h.request();
    h.reply(HttpResponse {
        id: feed.id,
        status: 200,
        body: books_feed("Shelf", None, &[("urn:a", "Alpha", "a.epub")]).into_bytes(),
        url: Some(String::from("https://mirror.example/new/place/feed")),
        ..HttpResponse::default()
    });
    assert_eq!(h.request().url, "https://mirror.example/new/place/a.epub");
}

#[test]
fn truncated_and_garbage_feeds_end_the_sync_quietly() {
    let bodies: [&[u8]; 5] = [
        b"",
        b"<feed><entry><title>Cut",
        b"\xff\xfe\x00<\x00f\x00e\x00e\x00d",
        b"{\"not\": \"a feed\"}",
        b"<!DOCTYPE feed [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\">]><feed><title>&b;&b;&b;</title></feed>",
    ];
    for body in bodies {
        let mut h = Harness::new(one_catalog());
        h.run(SYNC);
        let feed = h.request();
        h.answer(&feed, 200, body);
        assert!(h.requests().is_empty(), "{body:?}");
        assert!(books(&h.storage).is_empty());
        // Over: it can start again.
        h.run(SYNC);
        assert_eq!(h.request().url, SHELF);
    }
}
