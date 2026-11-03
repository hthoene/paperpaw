//! The first-run setup's languages (#116) with the real kernel on an X4
//! that loads code plugins: the list of languages the library has packs
//! for, what each takes, what is chosen fetched letters first, a choice
//! that does not fit said before anything is fetched, and the page without
//! a connection.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{LIST, Shopping};
use paperpaw_sdk::native::{Fingerprint, Native};
use paperpaw_sdk::{Id, Instant, Params, Storage};

/// The build of this PaperPaw.
const BUILD: Fingerprint = Fingerprint::of(paperpaw_sdk::native::INTERFACE, 0x4567_89ab_cdef);

fn native() -> Native {
    let mut native = Native::new("code.ppn");
    native.fingerprint = BUILD;
    native
}

/// A data pack of the library's list.
fn data(id: &str, name: &str, size: u64) -> String {
    format!(
        r#"{{"id": "{id}", "name": "{name}", "summary": "Pack.", "group": "look",
  "author": "PaperPaw", "licence": "OFL-1.1", "kind": "data",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 3, "size": {size}}}}}"#
    )
}

/// A code pack (hyphenation) with a build for the X4.
fn code(id: &str, name: &str, size: u64) -> String {
    format!(
        r#"{{"id": "{id}", "name": "{name}", "summary": "Pack.", "group": "reading",
  "author": "PaperPaw", "licence": "Apache-2.0", "kind": "code",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 100,
    "builds": [{{"device": "x4", "abi": "{BUILD}", "url": "{id}/1.0.0/x4.ppn",
                 "size": {size}, "sha256": "{}"}}]}}}}"#,
        "0".repeat(64)
    )
}

/// The library as of today: German, Norwegian and Russian hyphenation,
/// the Cyrillic and Greek letters, and Arabic with its letters.
fn list() -> String {
    let packs = [
        code("hyphenation-de", "Hyphenation: German", 370_500),
        code("hyphenation-nb", "Hyphenation: Norwegian", 271_616),
        code("hyphenation-ru", "Hyphenation: Russian", 94_228),
        data("script-cyrillic", "Cyrillic letters", 1_283_379),
        data("script-greek", "Greek letters", 3_670_464),
        data("script-arabic", "Arabic letters", 900_000),
        data("language-ar", "Arabic (user interface)", 210_000),
        // Only for books that need them: not a language of its own.
        data(
            "script-korean-extra",
            "Korean letters, rare syllables",
            30_000_000,
        ),
    ];
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, packs.join(","))
}

/// The page on an X4 whose shell kept the room as `room` says, opened as
/// the setup opens it, the library's list there.
fn page(room: Option<&str>) -> Shopping {
    let storage = common::Shared::default();
    if let Some(room) = room {
        storage
            .write(".paperpaw/code-room.txt", room.as_bytes())
            .unwrap();
    }
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list(), storage);
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    shopping
}

#[test]
fn the_languages_are_listed_with_what_each_takes() {
    let mut shopping = page(None);
    let shown = shopping.shown();
    // By name, each with what it brings and the size of the lot.
    for row in [
        "Arabic",
        "German",
        "Greek",
        "Norwegian",
        "Russian",
        "letters, menus · 1.1 MB",
        "hyphenation · 362 KB",
        "letters · 3.5 MB",
        "letters, hyphenation · 1.3 MB",
    ] {
        assert!(shown.contains(row), "{row}: {shown}");
    }
    // Not Korean: only its rare syllables are listed.
    assert!(!shown.contains("Korean"), "{shown}");
    assert_eq!(shown.matches("Hyphenation").count(), 0, "{shown}");
    shopping.check("languages-list");
}

#[test]
fn what_is_chosen_is_fetched_letters_first_and_only_by_download() {
    let mut shopping = page(None);
    shopping.choose("German");
    shopping.choose("Russian");
    shopping.choose("Ukrainian");
    let shown = shopping.shown();
    // Russian and Ukrainian share their letters: they are in the lot once.
    assert!(shown.contains("Download 1.7 MB"), "{shown}");
    // Leaving the page fetches nothing.
    assert!(shopping.requests().is_empty());
    // Down to the button, and *Download*.
    shopping.choose("Download");
    let asked = shopping.requests();
    let urls: Vec<&str> = asked.iter().map(|c| c.url.as_str()).collect();
    // The first of the queue: the letters Russian needs.
    assert!(
        urls.iter().any(|u| u.contains("/script-cyrillic.json")),
        "{urls:?}"
    );
    let queue = paperpaw_sdk::read_file(
        shopping.storage.0.as_ref(),
        ".paperpaw/plugins/store/queue.txt",
        4096,
    )
    .map(|bytes| String::from_utf8(bytes).unwrap())
    .unwrap_or_default();
    assert!(
        queue.find("script-cyrillic").unwrap() < queue.find("hyphenation-ru").unwrap(),
        "{queue}"
    );
    assert!(queue.contains("hyphenation-de"), "{queue}");
}

#[test]
fn a_choice_that_does_not_fit_says_so_and_has_no_download() {
    // 400 kB of room for code: German hyphenation (362 KB) fits, with
    // Norwegian (265 KB) it does not.
    let mut shopping = page(Some("409600 0 4096 83 0 0 3000000\n"));
    shopping.choose("German");
    assert!(shopping.shown().contains("Download 362 KB"));
    shopping.choose("Norwegian");
    let shown = shopping.shown();
    assert!(shown.contains("room for code plugins"), "{shown}");
    assert!(!shown.contains("| Download"), "{shown}");
}

#[test]
fn without_a_connection_the_page_says_so_and_the_setup_goes_on() {
    let (mut shopping, _) = Shopping::asked_for(&list(), false);
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    let shown = shopping.shown();
    assert!(shown.contains("No connection"), "{shown}");
    assert!(
        shown.contains("in the settings later, under Languages"),
        "{shown}"
    );
    assert!(shopping.requests().is_empty());
}

#[test]
fn the_list_is_fetched_where_the_store_has_none_yet() {
    // The store's start got no list it could keep, as on a first start.
    let (mut shopping, _) = Shopping::asked_for("{}", true);
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    assert!(shopping.shown().contains("Loading"), "{}", shopping.shown());
    // The list asked for as the store's start asks, without its order.
    let url = &LIST[..LIST.rfind("&order").unwrap()];
    shopping.serve(url, list().as_bytes());
    let shown = shopping.shown();
    // No code loaded here: the data packs only.
    assert!(shown.contains("Greek"), "{shown}");
}
