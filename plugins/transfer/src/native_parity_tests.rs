//! The native decisions (`native_transfer`) against the interface-6 ones
//! (`link`, `web`, `cable`): equal on the same input, and refusing instead of
//! aborting when memory runs short.
use super::{Offer as Legacy, is_book_name as legacy_book_name, name_key as legacy_key};
use crate::fail_alloc::sweep;
use crate::native_transfer::{self as native, Offer};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::wifi::WifiState;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn legacy_offer(text: &str) -> Option<Legacy> {
    let storage = MemoryStorage::new().with("offer.txt", text.as_bytes());
    Legacy::read(&storage, "offer.txt")
}

/// Every line of an offer's grammar, good and bad.
fn lines() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in [
        "plugin stats",
        "plugin Stats",
        "plugin ",
        "plugin ../x",
        "plugin a.b",
        "plugin stats extra",
        "manifest.toml 12 SHA",
        "code.ppn 4096 SHA",
        "sub/a.txt 3 SHA",
        "../a.txt 3 SHA",
        "/a.txt 3 SHA",
        "a.txt 3 SHA",
        "a.txt x SHA",
        "a.txt 3 zz",
        "a.txt 3",
        "a.txt",
        "b.txt 18446744073709551615 SHA",
        "book Moby Dick.epub 1000 SHA",
        "book a.epub 1 SHA",
        "book .hidden 1 SHA",
        "book a/b.epub 1 SHA",
        "book a:b.epub 1 SHA",
        "book  1 SHA",
        "book x",
        "book",
        "stray words here",
        "   ",
        "",
        "# comment",
    ] {
        out.push(line.replace("SHA", SHA));
    }
    out
}

fn offers() -> Vec<String> {
    let lines = lines();
    let mut out = Vec::new();
    for a in &lines {
        out.push(a.clone());
        for b in &lines {
            out.push(format!("{a}\n{b}"));
            out.push(format!("{a}\r\n{b}\r\n"));
        }
    }
    let plugin = format!("plugin stats\nmanifest.toml 12 {SHA}\ncode.ppn 10 {SHA}");
    out.push(plugin.clone());
    out.push(format!("{plugin}\nbook one.epub 5 {SHA}"));
    out.push(format!("{plugin}\nplugin other\na.txt 1 {SHA}"));
    out.push(format!("book one.epub 5 {SHA}\n{plugin}"));
    out.push(format!("plugin stats\n{}", "x".repeat(400)));
    out.push(format!("book {} 1 {SHA}", "n".repeat(200)));
    out.push(format!("book {} 1 {SHA}", "n".repeat(201)));
    // Past the books an offer holds, the lines an offer has, the plugin files
    // a plugin has, and the bytes an offer is.
    for count in [63, 64, 65, 100] {
        let mut text = String::new();
        for i in 0..count {
            text.push_str(&format!("book b{i}.epub {i} {SHA}\n"));
        }
        out.push(text);
    }
    for count in [63, 64, 65] {
        let mut text = String::from("plugin stats\n");
        for i in 0..count {
            text.push_str(&format!("f{i}.txt {i} {SHA}\n"));
        }
        out.push(text);
    }
    let mut big = String::new();
    for _ in 0..(native::MAX_FILES + 2) {
        big.push('\n');
    }
    out.push(big);
    out
}

#[test]
fn an_offer_is_read_as_the_interface_6_offer_is() {
    for text in offers() {
        let old = legacy_offer(&text);
        let new = Offer::parse(&text).unwrap();
        match (old, new) {
            (None, None) => {}
            (Some(old), Some(new)) => {
                assert_eq!(old.plugin, new.plugin, "{text:?}");
                assert_eq!(old.books, new.books, "{text:?}");
                assert_eq!(old.named, new.named, "{text:?}");
                assert_eq!(old.size(), new.size(), "{text:?}");
                assert_eq!(old.code(), new.code(), "{text:?}");
            }
            (old, new) => panic!("{text:?}: {old:?} / {new:?}"),
        }
    }
}

#[test]
fn what_was_offered_is_found_as_before() {
    let text = format!(
        "plugin stats\nmanifest.toml 12 {SHA}\nbook Moby Dick.epub 7 {SHA}\nbook two.epub 9 {SHA}"
    );
    let old = legacy_offer(&text).unwrap();
    let new = Offer::parse(&text).unwrap().unwrap();
    for (id, path) in [
        ("stats", "manifest.toml"),
        ("stats", "other.txt"),
        ("other", "manifest.toml"),
    ] {
        assert_eq!(old.plugin_file(id, path), new.plugin_file(id, path));
    }
    for name in ["Moby Dick.epub", "two.epub", "three.epub", ""] {
        assert_eq!(old.book(name), new.book(name), "{name}");
    }
}

#[test]
fn names_and_keys_are_those_of_interface_6() {
    for name in [
        "a.epub",
        "",
        ".hidden",
        "a/b",
        "a\\b",
        "a:b",
        "a\0b",
        "tab\tname",
        "ünï.epub",
        &"n".repeat(200),
        &"n".repeat(201),
    ] {
        assert_eq!(native::is_book_name(name), legacy_book_name(name), "{name}");
        assert_eq!(native::name_key(name), legacy_key(name), "{name}");
    }
    for path in [
        "paperpaw.ini",
        "PAPERPAW.INI",
        "PaperPaw.Ini",
        "a/paperpaw.ini",
        "",
    ] {
        assert_eq!(
            native::read_only(path),
            crate::web::read_only(path),
            "{path}"
        );
    }
}

fn online(name: Option<&str>) -> WifiState {
    WifiState::Online {
        ssid: "home".into(),
        address: "192.168.1.20".into(),
        name: name.map(String::from),
    }
}

#[test]
fn the_addresses_are_those_of_interface_6() {
    let hosting = WifiState::Hosting {
        ssid: "PaperPaw".into(),
        address: "192.168.4.1".into(),
        guests: 1,
    };
    let states = [
        Some(online(Some("paperpaw.local"))),
        Some(online(Some(""))),
        Some(online(None)),
        Some(hosting),
        Some(WifiState::Off),
        None,
    ];
    for state in &states {
        for address in [
            "192.168.1.20:8080",
            "192.168.1.20",
            "192.168.4.1:80",
            "host:",
        ] {
            for key in ["abc", ""] {
                let old = crate::link::urls(state.as_ref(), address, key);
                let reach = state.as_ref().and_then(WifiState::reach);
                let new = native::urls(reach, address, key).unwrap();
                assert_eq!(old, new, "{state:?} {address} {key}");
            }
        }
    }
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let text = format!(
        "plugin stats\nmanifest.toml 12 {SHA}\nbook Moby Dick.epub 7 {SHA}\nbook two.epub 9 {SHA}"
    );
    sweep(|| Offer::parse(&text).map(|offer| offer.is_some()));
    sweep(|| native::urls(Some("paperpaw.local"), "192.168.1.20:8080", "abc").map(|u| u.0.len()));
    sweep(|| native::urls(None, "192.168.1.20:8080", "abc").map(|u| u.0.len()));
}
