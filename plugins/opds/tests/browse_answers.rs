//! Browsing a catalog against answers servers really give, and hostile
//! ones: HTTP errors, redirects, no network, truncated and malformed
//! feeds, deep nesting, entities, thousands of entries, `javascript:`
//! links, searches (by template and by OpenSearch description) and
//! downloads whose titles and addresses try to leave the catalog's
//! folder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "harness/mod.rs"]
mod common;

use common::{DIR, Harness, Shared};
use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_plugin_opds::OPEN;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::{Params, Storage};

/// The catalog the list starts with.
const START: &str = "https://www.gutenberg.org/ebooks.opds/";
/// Where the catalog's downloads go (no files plugin: the card's top).
const FOLDER: &str = "Project Gutenberg";

/// The start catalog opened: its feed is asked for.
fn browse() -> (Harness, HttpCall) {
    let mut h = Harness::new(Shared::default());
    h.run(OPEN);
    h.run(actions::CONFIRM);
    let call = h.request();
    assert_eq!(call.url, START);
    (h, call)
}

fn feed(entries: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"><title>Shelf</title>{entries}</feed>"#
    )
}

fn book(title: &str, href: &str) -> String {
    format!(
        r#"<entry><title>{title}</title><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="{href}"/></entry>"#
    )
}

fn list(title: &str, href: &str) -> String {
    format!(
        r#"<entry><title>{title}</title><link rel="subsection" type="application/atom+xml;profile=opds-catalog" href="{href}"/></entry>"#
    )
}

#[test]
fn http_errors_are_told_with_their_status() {
    for (status, said) in [
        (404, "error 404"),
        (500, "error 500"),
        (503, "error 503"),
        (403, "password"),
        (401, "password"),
    ] {
        let (mut h, call) = browse();
        h.answer(&call, status, b"<html>error page</html>");
        let shown = h.said();
        assert!(shown.contains(said), "{status}: {shown}");
        // What was loaded is not kept.
        assert!(!h.storage.0.exists(&format!("{DIR}/browse.xml")));
    }
}

#[test]
fn no_answer_names_the_host() {
    let (mut h, call) = browse();
    h.reply(HttpResponse::failed(
        call.id,
        paperpaw_sdk::HttpError::Timeout,
        "timed out",
    ));
    let shown = h.said();
    assert!(
        shown.contains("www.gutenberg.org could not be reached"),
        "{shown}"
    );
}

#[test]
fn a_redirected_feed_resolves_its_links_where_it_landed() {
    let (mut h, call) = browse();
    h.reply(HttpResponse {
        id: call.id,
        status: 200,
        body: feed(&list("Popular", "popular.opds")).into_bytes(),
        url: Some(String::from("https://mirror.example/opds/root.xml")),
        ..HttpResponse::default()
    });
    assert!(h.shows("Popular"));
    h.run(actions::CONFIRM);
    assert_eq!(h.request().url, "https://mirror.example/opds/popular.opds");
}

#[test]
fn malformed_feeds_are_not_catalogs_and_do_not_panic() {
    let bodies: [&[u8]; 7] = [
        b"",
        b"<feed",
        b"<feed xmlns=\"http://www.w3.org/2005/Atom\"><entry><title>Cut in the mid",
        b"\x00\x01\x02\xff\xfe binary",
        b"<html><body><p>Hello</p></body></html>",
        b"<feed><entry><link rel=\"subsection\" type=\"application/atom+xml\"/></entry></feed>",
        b"{\"metadata\": {}, \"navigation\": 3}",
    ];
    for body in bodies {
        let (mut h, call) = browse();
        h.answer(&call, 200, body);
        let shown = h.said();
        assert!(
            shown.contains("not an OPDS catalog"),
            "{:?}: {shown}",
            String::from_utf8_lossy(body)
        );
    }
}

#[test]
fn a_truncated_feed_keeps_the_entries_that_came() {
    let (mut h, call) = browse();
    let whole = feed(&format!(
        "{}{}",
        list("First list", "/a"),
        list("Second list", "/b")
    ));
    let cut = &whole[..whole.find("Second").unwrap()];
    h.answer(&call, 200, cut.as_bytes());
    let shown = h.said();
    assert!(shown.contains("First list"), "{shown}");
    assert!(!shown.contains("Second"), "{shown}");
}

#[test]
fn deep_nesting_and_entity_bombs_are_read_without_harm() {
    let (mut h, call) = browse();
    let deep = format!(
        "{}{}{}",
        "<div>".repeat(20_000),
        list("Deep &amp; fine", "/deep"),
        "</div>".repeat(20_000)
    );
    let bomb = r#"<!DOCTYPE feed [<!ENTITY a "aaaaaaaaaaaaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"><!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">]>"#;
    let body = format!(
        "{bomb}<feed xmlns=\"http://www.w3.org/2005/Atom\"><title>&c;&c;&c;</title>{deep}{}</feed>",
        list("&c; &lt;b&gt; &#128512; &#xD800; &#99999999999;", "/odd")
    );
    h.answer(&call, 200, body.as_bytes());
    let shown = h.shown();
    assert!(shown.iter().any(|l| l.contains("Deep & fine")), "{shown:?}");
    // Entities are not expanded into thousands of letters.
    assert!(shown.iter().all(|l| l.len() < 2000), "{shown:?}");
    assert!(shown.iter().any(|l| l.contains("<b>")), "{shown:?}");
}

#[test]
fn thousands_of_entries_are_cut_to_a_page_with_more() {
    let (mut h, call) = browse();
    let entries: String = (0..5000)
        .map(|n| list(&format!("List {n}"), &format!("/l/{n}")))
        .collect();
    let body = format!(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Huge</title><link rel="next" type="application/atom+xml" href="/page/2"/>{entries}</feed>"#
    );
    h.answer(&call, 200, body.as_bytes());
    assert!(h.shows("List 0"));
    // The last row (past the hundred entries kept) is *More*.
    h.run(actions::UP);
    let shown = h.said();
    assert!(shown.contains("More"), "{shown}");
    // Before it, the hundredth entry: the rest are not kept.
    h.run(actions::UP);
    let shown = h.said();
    assert!(shown.contains("List 99"), "{shown}");
    assert!(!shown.contains("List 100"), "{shown}");
    h.run(actions::DOWN);
    h.run(actions::CONFIRM);
    assert_eq!(h.request().url, "https://www.gutenberg.org/page/2");
}

#[test]
fn a_next_link_to_itself_is_only_followed_when_asked() {
    let (mut h, call) = browse();
    let body = format!(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Loop</title><link rel="next" type="application/atom+xml" href="{START}"/>{}</feed>"#,
        list("Only", "/only")
    );
    h.answer(&call, 200, body.as_bytes());
    assert!(h.requests().is_empty(), "nothing loads by itself");
    // *More* (the last row) loads the same page again, once.
    h.run(actions::UP);
    h.run(actions::CONFIRM);
    let again = h.request();
    assert_eq!(again.url, START);
    h.answer(&again, 200, body.as_bytes());
    assert!(h.requests().is_empty());
    // *Back* returns through the pages shown.
    h.run(actions::BACK);
    let back = h.request();
    assert_eq!(back.url, START);
}

#[test]
fn javascript_links_lead_nowhere_harmful() {
    let (mut h, call) = browse();
    let body = feed(&format!(
        "{}{}",
        list("Script list", "javascript:alert(1)"),
        book("Script book", "javascript:alert(2)")
    ));
    h.answer(&call, 200, body.as_bytes());
    for down in [0, 1] {
        for _ in 0..down {
            h.run(actions::DOWN);
        }
        h.run(actions::CONFIRM);
        for call in h.requests() {
            // The link is not made into an address on the catalog's host.
            assert!(!call.url.starts_with("https://www.gutenberg.org/javascript"));
            // The shell cannot fetch it.
            h.reply(HttpResponse::failed(
                call.id,
                paperpaw_sdk::HttpError::Refused,
                "unsupported scheme",
            ));
        }
        // The feed stays; nothing was saved.
        assert!(h.shows("Script list"));
    }
    let saved: Vec<String> = h
        .storage
        .files("")
        .into_iter()
        .filter(|p| !p.starts_with(".paperpaw/"))
        .collect();
    assert!(saved.is_empty(), "{saved:?}");
}

#[test]
fn hostile_downloads_land_in_the_catalogs_folder_with_safe_names() {
    let titles = [
        ("../../../../evil", "/ebooks/1.epub"),
        ("..", "../../../../../two.epub"),
        ("", "/dl/%2e%2e%2F%2e%2e%2Fthree.epub?x=/../y"),
        ("C:\\Windows\\system32", "/ebooks/4.epub"),
        (
            "A <b>bold</b> &amp; &quot;quoted&quot; | title?",
            "/ebooks/5.epub",
        ),
    ];
    for (title, href) in titles {
        let (mut h, call) = browse();
        h.answer(&call, 200, feed(&book(title, href)).as_bytes());
        h.run(actions::CONFIRM);
        let download = h.request();
        assert!(
            download.url.starts_with("https://www.gutenberg.org/"),
            "{}",
            download.url
        );
        h.answer(&download, 200, b"PK book");
        let saved: Vec<String> = h
            .storage
            .files("")
            .into_iter()
            .filter(|p| !p.starts_with(".paperpaw/"))
            .collect();
        assert_eq!(saved.len(), 1, "{title}: {saved:?}");
        let name = saved[0]
            .strip_prefix(&format!("{FOLDER}/"))
            .unwrap_or_else(|| panic!("{title}: {saved:?}"));
        assert!(
            !name.contains(['/', '\\', ':', '<', '>', '|', '?', '"']),
            "{name}"
        );
        assert!(!name.starts_with('.'), "{name}");
        assert!(name.ends_with(".epub"), "{name}");
        assert!(!h.storage.0.exists(&format!("{DIR}/download.tmp")));
        // And it offers to read it.
        assert!(h.shows("Read now"), "{title}");
    }
}

#[test]
fn a_failed_download_saves_nothing_and_says_why() {
    let (mut h, call) = browse();
    h.answer(
        &call,
        200,
        feed(&book("Alice", "/ebooks/11.epub")).as_bytes(),
    );
    h.run(actions::CONFIRM);
    let download = h.request();
    h.answer(&download, 404, b"<html>Not found</html>");
    let shown = h.said();
    assert!(shown.contains("error 404"), "{shown}");
    assert!(h.storage.files(FOLDER).is_empty());
    assert!(!h.storage.0.exists(&format!("{DIR}/download.tmp")));
    // The same book twice: the second gets a name of its own.
    h.run(actions::CONFIRM);
    let first = h.request();
    h.answer(&first, 200, b"PK one");
    h.run(actions::BACK);
    h.run(actions::CONFIRM);
    let second = h.request();
    h.answer(&second, 200, b"PK two");
    let saved = h.storage.files(FOLDER);
    assert_eq!(saved.len(), 2, "{saved:?}");
}

#[test]
fn entries_without_a_readable_format_say_so() {
    let (mut h, call) = browse();
    let body = feed(
        r#"<entry><title>Only PDF to buy</title><link rel="http://opds-spec.org/acquisition/buy" type="application/pdf" href="/x.pdf"/><link rel="http://opds-spec.org/acquisition" type="application/x-unknown" href="/x.bin"/></entry>"#,
    );
    h.answer(&call, 200, body.as_bytes());
    h.run(actions::CONFIRM);
    assert!(h.requests().is_empty());
    assert!(h.shows("formats can be read"));
}

#[test]
fn searches_by_template_encode_the_words() {
    let (mut h, call) = browse();
    let body = format!(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>S</title><link rel="search" type="application/atom+xml" href="/search?q={{searchTerms}}&amp;page={{startPage?}}"/>{}</feed>"#,
        list("x", "/x")
    );
    h.answer(&call, 200, body.as_bytes());
    // *Search* is the first row.
    h.run(actions::CONFIRM);
    assert!(h.requests().is_empty(), "the words are asked for first");
    h.send(
        "opds.terms",
        Typed::new(String::from("  Jane & \"Austen\"/../ ü  "), Params::new()),
    );
    assert_eq!(
        h.request().url,
        "https://www.gutenberg.org/search?q=Jane+%26+%22Austen%22%2F..%2F+%C3%BC&page="
    );
    // Nothing typed: nothing searched.
    h.run(actions::BACK);
    h.send("opds.terms", Typed::new(String::from("   "), Params::new()));
    assert!(h.requests().iter().all(|c| !c.url.contains("search")));
}

#[test]
fn searches_by_description_first_ask_where_to_search() {
    let (mut h, call) = browse();
    let body = format!(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>S</title><link rel="search" type="application/opensearchdescription+xml" href="/osd.xml"/>{}</feed>"#,
        list("x", "/x")
    );
    h.answer(&call, 200, body.as_bytes());
    h.run(actions::CONFIRM);
    let described = h.request();
    assert_eq!(described.url, "https://www.gutenberg.org/osd.xml");
    let osd = r#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
<Url type="text/html" template="https://www.gutenberg.org/html?q={searchTerms}"/>
<Url type="application/atom+xml" template="/opds/search?q={searchTerms}"/></OpenSearchDescription>"#;
    h.answer(&described, 200, osd.as_bytes());
    h.send(
        "opds.terms",
        Typed::new(String::from("Kafka"), Params::new()),
    );
    assert_eq!(
        h.request().url,
        "https://www.gutenberg.org/opds/search?q=Kafka"
    );
}

#[test]
fn a_description_without_an_opds_address_cannot_be_searched() {
    for (status, answer) in [
        (
            200,
            "<OpenSearchDescription><Url type=\"text/html\" template=\"/q={searchTerms}\"/></OpenSearchDescription>",
        ),
        (200, "garbage <<<"),
        (404, ""),
    ] {
        let (mut h, call) = browse();
        let body = format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>S</title><link rel="search" type="application/opensearchdescription+xml" href="/osd.xml"/>{}</feed>"#,
            list("x", "/x")
        );
        h.answer(&call, 200, body.as_bytes());
        h.run(actions::CONFIRM);
        let described = h.request();
        h.answer(&described, status, answer.as_bytes());
        let shown = h.said();
        let said = if status == 200 {
            "cannot be searched"
        } else {
            "error 404"
        };
        assert!(shown.contains(said), "{answer}: {shown}");
    }
}

#[test]
fn a_late_answer_for_another_page_is_ignored() {
    let (mut h, call) = browse();
    h.answer(
        &call,
        200,
        feed(&format!("{}{}", list("A", "/a"), list("B", "/b"))).as_bytes(),
    );
    h.run(actions::CONFIRM);
    let a = h.request();
    // B chosen before A came: A's answer is late.
    h.run(actions::DOWN);
    h.run(actions::CONFIRM);
    let b = h.request();
    assert!(b.url.ends_with("/b"), "{}", b.url);
    h.answer(&a, 200, feed(&list("Wrong", "/w")).as_bytes());
    h.answer(&b, 200, feed(&list("Right", "/r")).as_bytes());
    let shown = h.said();
    assert!(
        shown.contains("Right") && !shown.contains("Wrong"),
        "{shown}"
    );
}
