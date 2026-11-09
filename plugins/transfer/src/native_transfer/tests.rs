//! The decisions of the transfer without the host, pinned (they were compared
//! with the interface-6 code before it was removed).
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::fail_alloc::sweep;
use alloc::format;
use alloc::string::ToString;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sha() -> [u8; 32] {
    digest(SHA).unwrap()
}

fn parse(text: &str) -> Option<Offer> {
    Offer::parse(&text.replace("SHA", SHA)).unwrap()
}

#[test]
fn a_plugin_and_books_are_an_offer() {
    let offer =
        parse("plugin stats\nmanifest.toml 12 SHA\ncode.ppn 10 SHA\nbook Moby Dick.epub 7 SHA\r\nbook two.epub 9 SHA\n")
            .unwrap();
    let (id, files) = offer.plugin.as_ref().unwrap();
    assert_eq!(id, "stats");
    assert_eq!(files.len(), 2);
    assert_eq!(offer.books.len(), 2);
    assert_eq!(
        offer.named,
        [
            ("Moby Dick.epub".to_string(), 7),
            ("two.epub".to_string(), 9)
        ]
    );
    assert_eq!(offer.size(), Some(12 + 10 + 7 + 9));
    assert!(offer.code());
    assert_eq!(
        offer.plugin_file("stats", "manifest.toml"),
        Some((12, sha()))
    );
    assert_eq!(offer.plugin_file("stats", "other.txt"), None);
    assert_eq!(offer.plugin_file("other", "manifest.toml"), None);
    assert_eq!(offer.book("two.epub"), Some((9, sha())));
    assert_eq!(offer.book("three.epub"), None);
    assert_eq!(offer.book(""), None);
}

#[test]
fn a_plugin_without_code_has_none() {
    let offer = parse("plugin stats\nmanifest.toml 12 SHA\nsub/a.txt 3 SHA").unwrap();
    assert!(!offer.code());
    assert_eq!(offer.size(), Some(15));
}

#[test]
fn a_line_that_is_not_understood_or_may_not_be_written_refuses_the_offer() {
    for text in [
        "",
        "   ",
        "# comment",
        "plugin stats",
        "plugin Stats\nmanifest.toml 1 SHA",
        "plugin ../x\na.txt 1 SHA",
        "plugin a.b\na.txt 1 SHA",
        "plugin \na.txt 1 SHA",
        "plugin stats\nplugin other\na.txt 1 SHA",
        "a.txt 3 SHA",
        "plugin stats\n../a.txt 3 SHA",
        "plugin stats\n/a.txt 3 SHA",
        "plugin stats\n.hidden 3 SHA",
        "plugin stats\na.txt x SHA",
        "plugin stats\na.txt 3 zz",
        "plugin stats\na.txt 3",
        "plugin stats\na.txt",
        "plugin stats\nstray words here",
        "book .hidden 1 SHA",
        "book a/b.epub 1 SHA",
        "book a:b.epub 1 SHA",
        "book  1 SHA",
        "book x",
        "book",
    ] {
        assert!(parse(text).is_none(), "{text:?}");
    }
}

#[test]
fn an_offer_has_its_limits() {
    let books = |count: usize| {
        let mut text = String::new();
        for i in 0..count {
            text.push_str(&format!("book b{i}.epub {i} SHA\n"));
        }
        text
    };
    assert_eq!(parse(&books(MAX_BOOKS)).unwrap().books.len(), MAX_BOOKS);
    assert!(parse(&books(MAX_BOOKS + 1)).is_none());
    let files = |count: usize| {
        let mut text = String::from("plugin stats\n");
        for i in 0..count {
            text.push_str(&format!("f{i}.txt {i} SHA\n"));
        }
        text
    };
    assert!(parse(&files(install::MAX_FILES)).is_some());
    assert!(parse(&files(install::MAX_FILES + 1)).is_none());
    assert!(parse(&format!("book {} 1 SHA", "n".repeat(200))).is_some());
    assert!(parse(&format!("book {} 1 SHA", "n".repeat(201))).is_none());
    assert!(parse(&format!("plugin stats\n{}", "x".repeat(MAX_LINE + 1))).is_none());
    // Only so many lines are read, and so many bytes.
    assert!(parse(&"\n".repeat(MAX_FILES + 2)).is_none());
    assert!(
        Offer::parse(&"a".repeat(MAX_OFFER as usize + 1))
            .unwrap()
            .is_none()
    );
    // Only the first books are named.
    let many = parse(&books(NAMED + 3)).unwrap();
    assert_eq!((many.books.len(), many.named.len()), (NAMED + 3, NAMED));
}

#[test]
fn a_size_past_what_can_be_counted_has_none() {
    let offer = parse("book a.epub 18446744073709551615 SHA\nbook b.epub 1 SHA").unwrap();
    assert_eq!(offer.size(), None);
}

#[test]
fn names_and_keys_are_those_of_interface_6() {
    for (name, good) in [
        ("a.epub", true),
        ("ünï.epub", true),
        ("", false),
        (".hidden", false),
        ("a/b", false),
        ("a\\b", false),
        ("a:b", false),
        ("a\0b", false),
        ("tab\tname", false),
    ] {
        assert_eq!(is_book_name(name), good, "{name:?}");
    }
    assert!(is_book_name(&"n".repeat(200)));
    assert!(!is_book_name(&"n".repeat(201)));
    // The first 16 bytes of the name's SHA-256.
    let whole = paperpaw_sdk::digest::sha256::digest(b"a.epub");
    assert_eq!(name_key("a.epub"), whole[..16]);
    assert_ne!(name_key("a.epub"), name_key("b.epub"));
    for path in ["paperpaw.ini", "PAPERPAW.INI", "PaperPaw.Ini"] {
        assert!(read_only(path), "{path}");
    }
    for path in ["a/paperpaw.ini", "", "paperpaw.ini.bak", "notes.txt"] {
        assert!(!read_only(path), "{path}");
    }
}

#[test]
fn the_text_shows_the_name_and_the_code_keeps_the_number() {
    let (shown, scanned) = urls(Some("paperpaw.local"), "192.168.1.20:8080", "abc").unwrap();
    assert_eq!(shown, "http://paperpaw.local:8080/?k=abc");
    assert_eq!(scanned, "http://192.168.1.20:8080/?k=abc");
    // The usual port is not said.
    let (shown, scanned) = urls(Some("192.168.4.1"), "192.168.4.1", "abc").unwrap();
    assert_eq!(
        (shown.as_str(), scanned.as_str()),
        ("http://192.168.4.1/?k=abc", "http://192.168.4.1/?k=abc")
    );
    let (shown, scanned) = urls(None, "192.168.1.20:8080", "k").unwrap();
    assert_eq!(shown, scanned);
    assert_eq!(shown, "http://192.168.1.20:8080/?k=k");
    // An address with an empty port keeps it as it is.
    assert_eq!(urls(None, "host:", "").unwrap().0, "http://host:/?k=");
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let text = format!(
        "plugin stats\nmanifest.toml 12 {SHA}\nbook Moby Dick.epub 7 {SHA}\nbook two.epub 9 {SHA}"
    );
    sweep(|| Offer::parse(&text).map(|offer| offer.is_some()));
    sweep(|| urls(Some("paperpaw.local"), "192.168.1.20:8080", "abc").map(|u| u.0.len()));
    sweep(|| urls(None, "192.168.1.20:8080", "abc").map(|u| u.0.len()));
}
