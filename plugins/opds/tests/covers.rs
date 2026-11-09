//! Covers in a catalog's lists with the real kernel: loaded one after the
//! other on devices with the memory, decoded and kept so they need no
//! second download, failures not tried again for the feed shown, none at
//! all on small devices or when switched off, and an answer that comes
//! too late for the feed shown.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "harness/mod.rs"]
mod common;

use common::{DIR, Harness, Shared};
use paperpaw_core::HttpCall;
use paperpaw_plugin_opds::OPEN;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::image::PngWriter;
use paperpaw_sdk::{MemoryClass, MemoryStorage};

const START: &str = "https://www.gutenberg.org/ebooks.opds/";

/// A small gray PNG.
fn png() -> Vec<u8> {
    let storage = MemoryStorage::new();
    let mut writer = PngWriter::new(&storage, "c.png", 30, 40).unwrap();
    for y in 0..40u8 {
        writer.row(&storage, "c.png", &[y * 6; 30]).unwrap();
    }
    storage.get("c.png").unwrap()
}

fn cover_file(url: &str) -> String {
    format!("{DIR}/covers/{}.gray", paperpaw_sdk::digest::md5::of(url))
}

/// A feed of lists with covers at `/c/<n>.png`.
fn feed(names: &[&str]) -> String {
    let entries: String = names
        .iter()
        .enumerate()
        .map(|(n, name)| {
            format!(
                r#"<entry><title>{name}</title><link rel="subsection" type="application/atom+xml" href="/{name}"/><link rel="http://opds-spec.org/image/thumbnail" type="image/png" href="/c/{n}.png"/></entry>"#
            )
        })
        .collect();
    format!(r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>F</title>{entries}</feed>"#)
}

fn open(memory: MemoryClass, config: &str) -> (Harness, HttpCall) {
    let mut h = Harness::build(Shared::default(), memory, config);
    h.run(OPEN);
    h.run(actions::CONFIRM);
    let call = h.request();
    assert_eq!(call.url, START);
    (h, call)
}

#[test]
fn covers_come_one_after_another_and_are_kept() {
    let (mut h, call) = open(MemoryClass::Large, "");
    h.answer(&call, 200, feed(&["A", "B", "C"]).as_bytes());
    let first = h.request();
    assert_eq!(first.url, "https://www.gutenberg.org/c/0.png");
    assert!(first.save_to.is_some());
    h.answer(&first, 200, &png());
    let second = h.request();
    assert_eq!(second.url, "https://www.gutenberg.org/c/1.png");
    h.answer(&second, 404, b"");
    let third = h.request();
    // Not a picture at all.
    h.answer(&third, 200, b"<html>no image</html>");
    assert!(h.requests().is_empty(), "failures are not tried again");
    assert!(h.storage.0.get(&cover_file(&first.url)).is_some());
    assert!(h.storage.0.get(&cover_file(&second.url)).is_none());
    assert!(h.storage.0.get(&cover_file(&third.url)).is_none());
    assert!(h.storage.0.get(&format!("{DIR}/cover.tmp")).is_none());
    // Shown again: the kept cover needs no download.
    h.run(actions::BACK);
    h.run(actions::CONFIRM);
    let call = h.request();
    h.answer(&call, 200, feed(&["A", "B", "C"]).as_bytes());
    let next = h.request();
    assert_eq!(next.url, "https://www.gutenberg.org/c/1.png");
}

#[test]
fn small_devices_load_no_covers() {
    let (mut h, call) = open(MemoryClass::Small, "");
    h.answer(&call, 200, feed(&["A", "B"]).as_bytes());
    assert!(h.requests().is_empty());
}

#[test]
fn covers_switched_off_are_not_loaded() {
    let (mut h, call) = open(MemoryClass::Large, "[opds]\ncovers = false\n");
    h.answer(&call, 200, feed(&["A", "B"]).as_bytes());
    assert!(h.requests().is_empty());
}

#[test]
fn only_covers_on_the_page_shown_are_loaded() {
    let names: Vec<String> = (0..60).map(|n| format!("L{n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (mut h, call) = open(MemoryClass::Large, "");
    h.answer(&call, 200, feed(&names).as_bytes());
    let mut loaded = 0;
    while let Some(cover) = h.requests().pop() {
        loaded += 1;
        h.answer(&cover, 404, b"");
    }
    assert!(loaded > 0 && loaded < 60, "{loaded}");
}

#[test]
fn a_late_cover_of_the_feed_before_is_not_shown_for_another_entry() {
    let (mut h, call) = open(MemoryClass::Large, "");
    h.answer(&call, 200, feed(&["A"]).as_bytes());
    let old_cover = h.request();
    // On to A's feed before its cover came.
    h.run(actions::CONFIRM);
    let next = h.request();
    let other = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>A</title><entry><title>Book</title><link rel="subsection" type="application/atom+xml" href="/b"/><link rel="http://opds-spec.org/image/thumbnail" type="image/png" href="/other.png"/></entry></feed>"#;
    h.answer(&next, 200, other.as_bytes());
    let new_cover = h.request();
    assert_eq!(new_cover.url, "https://www.gutenberg.org/other.png");
    // The old cover comes now, then the new one.
    h.answer(&old_cover, 200, &png());
    h.answer(&new_cover, 200, &png());
    // The new entry's own cover is taken and kept.
    assert!(
        h.storage.0.get(&cover_file(&new_cover.url)).is_some(),
        "the cover of the entry shown was dropped"
    );
}
