//! What is typed and what servers send, and the lists of pages, at random:
//! the properties interface 6 had (its tests are gone with its code) hold
//! for the native rules too.

extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::storage::is_valid_path;
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use crate::native_store::{self as native, Entry, HISTORY, HISTORY_FILE};
use crate::native_web;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn anything_typed_is_an_address_or_a_search(input in ".{0,60}") {
        let url = native_web::address(&input).unwrap();
        let text = input.trim();
        prop_assert!(!url.contains(char::is_whitespace) || url == text, "{}", url);
        prop_assert!(url.contains(':'), "{}", url);
        if let Some(query) = url.strip_prefix("https://html.duckduckgo.com/html/?q=") {
            // The words go as a query, encoded.
            prop_assert!(!query.contains(['&', '#', '?', '/', ' ']), "{}", query);
        }
    }

    #[test]
    fn any_page_gives_a_title_or_none(data in prop::collection::vec(any::<u8>(), 0..2000)) {
        let storage = MemoryStorage::new().with("p.html", data);
        if let Some(title) = native_web::title(&storage, "p.html").unwrap() {
            prop_assert!(!title.is_empty());
            prop_assert!(!title.contains(['\n', '\t', '\r']));
            prop_assert_eq!(title.trim(), title.as_str());
        }
    }

    #[test]
    fn kept_pages_hold_their_address_inside_the_attribute(url in ".{0,80}") {
        let storage = MemoryStorage::new().with("tmp", b"<p>x</p>".to_vec());
        native_web::keep_page(&storage, "tmp", "page.html", &url).unwrap();
        let page = String::from_utf8(storage.get("page.html").unwrap()).unwrap();
        let first = page.lines().next().unwrap_or("");
        // `<base href="` ... `">`: no quote in between ends it early.
        let inner = first
            .strip_prefix("<base href=\"")
            .and_then(|r| r.strip_suffix("\">"))
            .unwrap_or_else(|| panic!("{first}"));
        if !url.contains(['\n', '\r']) {
            prop_assert!(!inner.contains('"'), "{}", inner);
        }
        prop_assert!(page.ends_with("<p>x</p>"));
        prop_assert!(!storage.exists("tmp"));
    }

    #[test]
    fn page_names_are_stable_valid_names(url in ".{0,80}") {
        let name = native_web::page_name(&url).unwrap();
        prop_assert_eq!(&name, &native_web::page_name(&url).unwrap());
        let path = format!("pages/{name}");
        prop_assert!(is_valid_path(&path), "{}", path);
        prop_assert_eq!(name.len(), 21);
    }

    #[test]
    fn lists_round_trip(list in prop::collection::vec((text(), text()), 0..8)) {
        let entries: Vec<Entry> = list.iter().map(|(t, u)| Entry::new(t, u).unwrap()).collect();
        let storage = MemoryStorage::new();
        native::save(&storage, "l.txt", &entries).unwrap();
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        // A byte order mark that starts the file is read as one, not as
        // part of the first title.
        let expected: Vec<Entry> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.url.is_empty())
            .map(|(i, e)| {
                let title = clean(&e.title);
                let title = match title.strip_prefix('\u{feff}') {
                    Some(rest) if i == 0 => String::from(rest),
                    _ => title,
                };
                Entry::new(&title, &clean(&e.url)).unwrap()
            })
            .collect();
        prop_assert_eq!(native::load(&storage, "l.txt").unwrap().unwrap(), expected);
    }

    #[test]
    fn the_history_stays_within_its_limit(urls in prop::collection::vec(0u8..50, 1..80)) {
        let storage = MemoryStorage::new();
        let mut last = Vec::new();
        for n in &urls {
            let url = format!("https://h.example/{n}");
            storage.write(&native::page_path(&url).unwrap(), b"page").unwrap();
            last = native::remember(&storage, Entry::new("T", &url).unwrap()).unwrap().unwrap();
        }
        prop_assert!(last.len() <= HISTORY);
        let first = format!("https://h.example/{}", urls[urls.len() - 1]);
        prop_assert_eq!(&last[0].url, &first);
        // Each page once; the pages kept are those listed.
        let mut seen = last.iter().map(|e| e.url.clone()).collect::<Vec<_>>();
        seen.sort();
        seen.dedup();
        prop_assert_eq!(seen.len(), last.len());
        for n in 0u8..50 {
            let url = format!("https://h.example/{n}");
            let listed = last.iter().any(|e| e.url == url);
            let visited = urls.contains(&n);
            prop_assert_eq!(storage.exists(&native::page_path(&url).unwrap()), listed && visited);
        }
    }
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![".{0,40}", "[\t\r\n a-z]{0,8}"]
}

#[test]
fn a_title_far_into_a_page_is_not_looked_for() {
    let page = format!("{}<title>Late</title>", " ".repeat(20_000));
    let storage = MemoryStorage::new().with("p.html", page);
    assert_eq!(native_web::title(&storage, "p.html").unwrap(), None);
}

#[test]
fn script_and_data_addresses_are_searched_for() {
    // Only `http://` and `https://` are taken as they are: other schemes are
    // words to search for, never loaded.
    for typed in [
        "javascript:alert(1)",
        "data:text/html,x",
        "file:///etc/passwd",
        "a:b c",
    ] {
        assert!(
            native_web::address(typed)
                .unwrap()
                .starts_with("https://html.duckduckgo.com/html/?q="),
            "{typed}"
        );
    }
}

/// A host with a port typed without a scheme is an address, not a scheme to
/// load as it is.
#[test]
fn a_host_with_a_port_is_an_address() {
    assert_eq!(
        native_web::address("calibre.local:8080").unwrap(),
        "https://calibre.local:8080"
    );
    assert_eq!(
        native_web::address("nas.example.org:5000/books").unwrap(),
        "https://nas.example.org:5000/books"
    );
}

/// A list that cannot be read is no list at all, and the history is not
/// written over it.
#[test]
fn an_unreadable_history_is_kept() {
    let kept = b"Caf\xe9\thttps://c.example/\n".to_vec();
    let storage = MemoryStorage::new().with(HISTORY_FILE, kept.clone());
    assert!(native::load(&storage, HISTORY_FILE).is_err());
    assert_eq!(
        native::remember(&storage, Entry::new("T", "https://t.example/").unwrap()),
        Ok(None)
    );
    assert_eq!(storage.get(HISTORY_FILE), Some(kept));
}
