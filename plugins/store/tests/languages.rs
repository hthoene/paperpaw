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

use common::{BuiltInOff, LIST, Shopping};
use paperpaw_platform::{Battery, SystemStatus};
use paperpaw_plugin_power::Power;
use paperpaw_plugin_wifi::Wifi;
use paperpaw_sdk::builtin::settings;
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

/// The library as Android has it: the hyphenation pack is built in but
/// switched off, so it is switched on and never fetched.
fn built_in_list() -> String {
    format!(
        r#"{{"format": 2, "plugins": [
 {{"id": "hyphenation-de", "name": "Hyphenation: German", "summary": "Pack.",
   "group": "reading", "author": "PaperPaw", "licence": "Apache-2.0", "kind": "code",
   "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 100}}}},
 {}]}}"#,
        data("script-greek", "Greek letters", 3_670_464)
    )
}

#[test]
fn a_built_in_pack_is_switched_on_and_never_fetched() {
    let mut shopping = Shopping::custom(
        |b| b.plugin_off(BuiltInOff("hyphenation-de")),
        common::Shared::default(),
        &built_in_list(),
        true,
    );
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    let shown = shopping.shown();
    // Nothing to download for German: built in.
    assert!(shown.contains("German, hyphenation · built in"), "{shown}");
    shopping.choose("German");
    let shown = shopping.shown();
    assert!(shown.contains("Nothing to download"), "{shown}");
    assert!(shown.contains("| Add"), "{shown}");
    shopping.choose("Add");
    assert!(shopping.requests().is_empty());
    // Switched on, and said.
    let shown = shopping.shown();
    assert!(
        shown.contains("The languages chosen are switched"),
        "{shown}"
    );
}

/// A battery at 5 % puts Automatic in Offline: the page offers to go
/// online, in its own layout, and *Go online* fetches the list.
#[test]
fn offline_mode_offers_to_go_online_in_place() {
    let mut shopping = Shopping::custom(|b| b.plugin(Power), common::Shared::default(), "{}", true);
    shopping.now += 60_000;
    shopping.kernel.set_status(
        Instant(shopping.now),
        SystemStatus::new(None, Some(Battery::new(5, false, None))),
    );
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    // The list is refused in Offline: the offer shows.
    let _ = shopping.requests();
    let shown = shopping.shown();
    assert!(shown.contains("Go online"), "{shown}");
    assert!(shown.contains("Not now"), "{shown}");
}

/// Where PaperPaw runs the radio, the page without a list offers the
/// Wi-Fi page, and opens it.
#[test]
fn without_a_connection_the_wifi_page_is_one_press_away() {
    let mut shopping = Shopping::custom(
        |b| b.wifi(true).plugin(Wifi).network(false),
        common::Shared::default(),
        &list(),
        false,
    );
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    let shown = shopping.shown();
    assert!(shown.contains("No connection"), "{shown}");
    assert!(shown.contains("| Wi-Fi"), "{shown}");
    shopping.run(paperpaw_sdk::builtin::actions::CONFIRM);
    let shown = shopping.shown();
    assert!(!shown.contains("No connection"), "{shown}");
}

/// What was chosen shows where it is: waiting for its turn, then
/// downloading, the line beside the button counts the languages left.
#[test]
fn the_page_shows_how_far_the_downloads_are() {
    let mut shopping = page(None);
    shopping.choose("Greek");
    shopping.choose("Download");
    // The setup goes on; the page opened again says what is going on.
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.languages"),
        Params::new(),
    );
    let shown = shopping.shown();
    assert!(
        shown.contains("Greek, Waiting for its turn") || shown.contains("Greek, Downloading"),
        "{shown}"
    );
    assert!(shown.contains("1 language still downloading"), "{shown}");
    // Not chosen a second time while it goes on: no Download for it.
    assert!(!shown.contains("| Download"), "{shown}");
}

/// A language pack that came: PaperPaw asks before the menus change,
/// once; *Switch* changes the language, *Not now* leaves it.
fn with_language_pack_installed() -> Shopping {
    let storage = common::Shared::default();
    storage
        .write(
            "plugins/language-ar/plugin.ini",
            b"[plugin]\nid = language-ar\nname = Arabic\nversion = 1.0.0\n",
        )
        .unwrap();
    storage
        .write(
            "plugins/language-ar/i18n/ar.ini",
            b"[meta]\nlanguage = ar\nname = Arabic\n",
        )
        .unwrap();
    storage
        .write(
            ".paperpaw/plugins/store/languages-chosen.txt",
            b"script-arabic\nlanguage-ar",
        )
        .unwrap();
    let mut shopping = Shopping::custom(|b| b, storage, &list(), true);
    shopping.kernel.send_event(
        Instant(shopping.now + 100),
        paperpaw_sdk::builtin::events::PLUGINS_CHANGED.id().clone(),
        Params::new(),
    );
    shopping.now += 200;
    shopping
}

#[test]
fn a_menu_pack_that_came_is_switched_to_only_when_the_reader_says_so() {
    let mut shopping = with_language_pack_installed();
    let shown = shopping.shown();
    assert!(shown.contains("PaperPaw is now available in"), "{shown}");
    assert_ne!(
        shopping.kernel.settings().read(&settings::LANGUAGE),
        "ar",
        "not before the answer"
    );
    shopping.choose("Switch");
    assert_eq!(shopping.kernel.settings().read(&settings::LANGUAGE), "ar");
}

#[test]
fn not_now_leaves_the_language_and_it_is_not_asked_again() {
    let mut shopping = with_language_pack_installed();
    shopping.choose("Not now");
    assert_ne!(shopping.kernel.settings().read(&settings::LANGUAGE), "ar");
    shopping.kernel.send_event(
        Instant(shopping.now + 100),
        paperpaw_sdk::builtin::events::PLUGINS_CHANGED.id().clone(),
        Params::new(),
    );
    assert!(
        !shopping.shown().contains("PaperPaw is now available"),
        "{}",
        shopping.shown()
    );
}
