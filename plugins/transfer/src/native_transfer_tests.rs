//! The transfer's own decisions (`native_transfer`): the addresses, the names,
//! the offers of a computer, and a refused allocation ending in `NoMemory`
//! instead of aborting.
use super::{Offer, is_book_name, name_key, read_only, urls};
use crate::fail_alloc::sweep;
use alloc::format;
use alloc::string::String;
use paperpaw_sdk::native::Error;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn parse(text: &str) -> Option<Offer> {
    Offer::parse(&text.replace("SHA", SHA)).unwrap()
}

#[test]
fn an_offer_names_a_plugin_and_books() {
    let offer = parse(
        "plugin stats\nmanifest.toml 12 SHA\ncode.ppn 10 SHA\nbook Moby Dick.epub 7 SHA\r\nbook two.epub 9 SHA\n",
    )
    .unwrap();
    assert_eq!(
        offer.plugin.as_ref().map(|(id, f)| (id.as_str(), f.len())),
        Some(("stats", 2))
    );
    assert_eq!(offer.books.len(), 2);
    assert_eq!(
        offer.named,
        [
            (String::from("Moby Dick.epub"), 7),
            (String::from("two.epub"), 9)
        ]
    );
    assert_eq!(offer.size(), Some(12 + 10 + 7 + 9));
    assert!(offer.code());
    assert!(
        offer
            .plugin_file("stats", "manifest.toml")
            .is_some_and(|(size, _)| size == 12)
    );
    assert_eq!(offer.plugin_file("stats", "other.txt"), None);
    assert_eq!(offer.plugin_file("other", "manifest.toml"), None);
    assert!(
        offer
            .book("Moby Dick.epub")
            .is_some_and(|(size, _)| size == 7)
    );
    assert_eq!(offer.book("two.epub").map(|(size, _)| size), Some(9));
    assert_eq!(offer.book("three.epub"), None);
    assert_eq!(offer.book(""), None);
}

#[test]
fn an_offer_with_a_line_that_is_not_understood_is_none() {
    for line in [
        "plugin ",
        "plugin ../x",
        "plugin Stats",
        "plugin a.b",
        "manifest.toml 12 SHA",
        "sub/a.txt 3 SHA",
        "book a.epub 1",
        "book .hidden 1 SHA",
        "book a/b.epub 1 SHA",
        "book a:b.epub 1 SHA",
        "book  1 SHA",
        "book x",
        "book",
        "stray words here",
        "# comment",
        "",
        "   ",
    ] {
        assert_eq!(parse(line), None, "{line:?}");
    }
    // Plugin files go with a plugin, only one plugin, only safe paths, and the
    // path is the first word of its own line.
    assert_eq!(parse("plugin a\nplugin b\nx.txt 1 SHA"), None);
    assert_eq!(parse("plugin a\n../x.txt 1 SHA"), None);
    assert_eq!(parse("plugin a\n/x.txt 1 SHA"), None);
    assert_eq!(parse("plugin a\nx.txt y SHA"), None);
    assert_eq!(parse("plugin a\nx.txt 1 zz"), None);
    // A plugin without a file offers nothing.
    assert_eq!(parse("plugin a"), None);
}

#[test]
fn an_offer_has_limits() {
    let mut books = String::new();
    for i in 0..64 {
        books.push_str(&format!("book b{i}.epub {i} SHA\n"));
    }
    assert_eq!(
        parse(&books).map(|o| (o.books.len(), o.named.len())),
        Some((64, 8))
    );
    books.push_str("book b64.epub 64 SHA\n");
    assert_eq!(parse(&books), None);
    assert_eq!(
        parse(&format!("book {} 1 SHA", "n".repeat(200))).map(|o| o.books.len()),
        Some(1)
    );
    assert_eq!(parse(&format!("book {} 1 SHA", "n".repeat(201))), None);
    assert_eq!(
        parse(&format!("book a.epub {} SHA\nbook b.epub 2 SHA", u64::MAX))
            .unwrap()
            .size(),
        None
    );
    // More bytes than an offer is.
    let big = "\n".repeat(usize::try_from(super::MAX_OFFER).unwrap() + 1);
    assert_eq!(Offer::parse(&big).unwrap(), None);
}

#[test]
fn names_and_keys() {
    for name in ["a.epub", "Moby Dick.epub", "ünï.epub", &"n".repeat(200)] {
        assert!(is_book_name(name), "{name}");
    }
    for name in [
        "",
        ".hidden",
        "a/b",
        "a\\b",
        "a:b",
        "a\0b",
        "tab\tname",
        &"n".repeat(201),
    ] {
        assert!(!is_book_name(name), "{name:?}");
    }
    assert_ne!(name_key("a.epub"), name_key("b.epub"));
    assert_eq!(name_key("a.epub"), name_key("a.epub"));
    for path in ["paperpaw.ini", "PAPERPAW.INI", "PaperPaw.Ini"] {
        assert!(read_only(path), "{path}");
    }
    for path in ["a/paperpaw.ini", "paperpaw.ini.bak", ""] {
        assert!(!read_only(path), "{path}");
    }
}

#[test]
fn the_addresses() {
    let shown = |reach, address| urls(reach, address, "abc").unwrap();
    let (text, qr) = shown(Some("paperpaw.local"), "192.168.1.20:8080");
    assert_eq!(text, "http://paperpaw.local:8080/?k=abc");
    assert_eq!(qr, "http://192.168.1.20:8080/?k=abc");
    let (text, qr) = shown(None, "192.168.1.20:8080");
    assert_eq!(
        (text.as_str(), qr.as_str()),
        (
            "http://192.168.1.20:8080/?k=abc",
            "http://192.168.1.20:8080/?k=abc"
        )
    );
    // The usual port is absent.
    let (text, _) = shown(Some("192.168.4.1"), "192.168.4.1");
    assert_eq!(text, "http://192.168.4.1/?k=abc");
    assert_eq!(urls(None, "h", "").unwrap().0, "http://h/?k=");
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let text = format!(
        "plugin stats\nmanifest.toml 12 {SHA}\nbook Moby Dick.epub 7 {SHA}\nbook two.epub 9 {SHA}"
    );
    sweep(|| Offer::parse(&text).map(|offer| offer.is_some()));
    sweep(|| urls(Some("paperpaw.local"), "192.168.1.20:8080", "abc").map(|u| u.0.len()));
    sweep(|| urls(None, "192.168.1.20:8080", "abc").map(|u| u.0.len()));
    let _: Result<(), Error> = Ok(());
}
