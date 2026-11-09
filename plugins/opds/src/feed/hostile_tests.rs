//! Feeds as hostile servers might send them: the readers of OPDS 1,
//! OPDS 2 and WebDAV listings never panic, keep at most a page of
//! entries with short texts, and resolve links against the feed.

extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::{Feed, MAX_ENTRIES, MAX_TEXT, read, read_webdav, search_template, search_url};

const BASE: &str = "https://cat.example/opds/root.xml";

fn stored(data: &[u8]) -> MemoryStorage {
    let storage = MemoryStorage::new();
    storage.write("f.xml", data).unwrap();
    storage
}

fn bounded(feed: &Feed) {
    assert!(feed.entries.len() <= MAX_ENTRIES);
    for e in &feed.entries {
        assert!(e.title.chars().count() <= MAX_TEXT + 1, "{}", e.title.len());
        assert!(e.summary.chars().count() <= MAX_TEXT + 1);
        assert!(e.feed.is_some() || !e.books.is_empty());
    }
}

/// Pieces of Atom feeds, put together at random.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("<feed xmlns=\"http://www.w3.org/2005/Atom\">")),
        Just(String::from("</feed>")),
        Just(String::from("<entry>")),
        Just(String::from("</entry>")),
        Just(String::from("<title>")),
        Just(String::from("</title>")),
        Just(String::from("<author><name>")),
        Just(String::from("</name></author>")),
        Just(String::from("<content type=\"text\">")),
        Just(String::from("</content>")),
        Just(String::from("<![CDATA[x]]>")),
        Just(String::from("&amp;&#0;&#xFFFFFFFF;&bogus;&")),
        Just(String::from(
            "<link rel=\"subsection\" type=\"application/atom+xml\" href=\"../../x\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"javascript:alert(1)\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/b.epub\"/>"
        )),
        Just(String::from("<link rel=\"next\" href=\"?page=2\"/>")),
        Just(String::from(
            "<link rel=\"search\" href=\"/s?q={searchTerms}\"/>"
        )),
        Just(String::from("<link href=\"\"/>")),
        Just(String::from("<!DOCTYPE x [<!ENTITY a \"aaaa\">]>")),
        Just(String::from("<div><div><div>")),
        "[a-z <>/\"=&;]{0,20}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn any_bytes_read_as_a_feed_without_panic(data in prop::collection::vec(any::<u8>(), 0..3000)) {
        let storage = stored(&data);
        bounded(&read(&storage, "f.xml", BASE));
        bounded(&read_webdav(&storage, "f.xml", BASE));
        bounded(&crate::json::read(&storage, "f.xml", BASE));
        let _ = search_template(&storage, "f.xml", BASE);
    }

    #[test]
    fn pieces_of_feeds_read_without_panic(parts in prop::collection::vec(fragment(), 0..80)) {
        let text = parts.concat();
        let feed = read(&stored(text.as_bytes()), "f.xml", BASE);
        bounded(&feed);
        // Links within the catalog are made absolute against it.
        for e in &feed.entries {
            let urls = e.feed.iter().chain(e.books.iter().map(|b| &b.url));
            for url in urls {
                prop_assert!(
                    url.starts_with("https://cat.example/") || url.starts_with("javascript:"),
                    "{}", url
                );
            }
        }
    }

    #[test]
    fn search_addresses_are_made_of_any_template(template in ".{0,60}", terms in ".{0,30}") {
        let url = search_url(&template, &terms);
        // The terms never bring their own syntax into the address.
        prop_assert!(!url.contains("{searchTerms}"), "{}", url);
    }
}

#[test]
fn a_hundred_thousand_entries_keep_a_page() {
    let mut text = String::from("<feed><title>Many</title>");
    for n in 0..100_000 {
        text.push_str(&format!(
            "<entry><title>{n}</title><link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/{n}.epub\"/></entry>"
        ));
    }
    text.push_str("</feed>");
    let feed = read(&stored(text.as_bytes()), "f.xml", BASE);
    assert_eq!(feed.entries.len(), MAX_ENTRIES);
    assert_eq!(feed.entries[99].books[0].url, "https://cat.example/99.epub");
}

#[test]
fn a_huge_title_is_cut() {
    let text = format!(
        "<feed><entry><title>{}</title><link rel=\"subsection\" type=\"application/atom+xml\" href=\"/x\"/></entry></feed>",
        "word ".repeat(50_000)
    );
    let feed = read(&stored(text.as_bytes()), "f.xml", BASE);
    assert_eq!(feed.entries.len(), 1);
    assert!(feed.entries[0].title.ends_with('…'));
    assert_eq!(feed.entries[0].title.chars().count(), MAX_TEXT + 1);
}

#[test]
fn webdav_entries_with_hostile_names_stay_titles() {
    let listing = r#"<d:multistatus xmlns:d="DAV:">
<d:response><d:href>/dav/Books/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/dav/Books/..%2F..%2Fescape.epub</d:href></d:response>
<d:response><d:href></d:href></d:response>
<d:response><d:href>https://elsewhere.example/x.epub</d:href></d:response>
<d:response><d:href>/dav/Books/Sub/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#;
    let feed = read_webdav(
        &stored(listing.as_bytes()),
        "f.xml",
        "https://cloud.example/dav/Books/",
    );
    let titles: Vec<&str> = feed.entries.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Sub", "escape", "x"], "{feed:?}");
    assert_eq!(
        feed.entries[1].books[0].url,
        "https://cloud.example/dav/Books/..%2F..%2Fescape.epub"
    );
}
