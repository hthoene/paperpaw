//! Plugins another plugin asks for (the reader's packs of letters for a
//! book) with the real kernel on an X4: the reader is asked in a dialog
//! that names them with their sizes, *Download* fetches them in the
//! background and the one who asked is told what came of it, once;
//! *Not now*, no connection and a plugin the library does not have are
//! answered at once.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Answers, Shopping};
use paperpaw_sdk::{Storage, Version};

const PAGE: &str = "https://paperpaw.org/api/v1/plugins/script-greek.json?device=x4&lang=en";

/// The list for the X4: the Greek letters, and another plugin.
fn list() -> String {
    r#"{"format": 2, "plugins": [
  {"id": "script-greek", "name": "Greek letters", "summary": "Greek books.",
    "group": "look", "author": "PaperPaw", "licence": "OFL-1.1", "kind": "data",
    "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 2900000}},
  {"id": "airy", "name": "Airy", "summary": "Wide margins.",
    "group": "reading", "author": "Someone", "licence": "MIT", "kind": "data",
    "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 300}}
]}"#
    .to_string()
}

const MANIFEST: &str =
    "[plugin]\nid = script-greek\nname = Greek letters\nversion = 1.0.0\nformats = 1\n";

/// The pack's page at the library: one file.
fn page() -> String {
    format!(
        r#"{{"format": 2, "plugin": {{"id": "script-greek", "name": "Greek letters",
  "summary": "Greek books.", "group": "look", "author": "PaperPaw",
  "licence": "OFL-1.1", "kind": "data",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 2900000,
    "files": [{{"path": "plugin.ini", "url": "script-greek/1.0.0/plugin.ini",
                "size": {}, "sha1": "{}"}}]}}}}}}"#,
        MANIFEST.len(),
        paperpaw_digest::sha1::hex(MANIFEST.as_bytes())
    )
}

/// What the one who asked was told, as `(installed, missing, declined,
/// error)`.
fn told(answers: &Answers) -> Vec<(String, String, bool, String)> {
    answers
        .borrow()
        .iter()
        .map(|a| {
            assert_eq!(a.by.as_str(), "reader");
            (
                a.installed.clone(),
                a.missing.clone(),
                a.declined,
                a.error.clone(),
            )
        })
        .collect()
}

#[test]
fn the_dialog_names_the_plugins_with_their_sizes() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    let shown = shopping.shown();
    // The question wraps over lines.
    assert!(shown.contains("This book needs more"), "{shown}");
    assert!(shown.contains("to show its text."), "{shown}");
    assert!(shown.contains("Greek letters (2.8 MB)"), "{shown}");
    assert!(
        shown.contains("Download") && shown.contains("Not now"),
        "{shown}"
    );
    // Nobody is told before the reader has answered.
    assert!(told(&answers).is_empty());
}

#[test]
fn not_now_is_told_as_declined() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.choose("Not now");
    assert_eq!(
        told(&answers),
        [(
            String::new(),
            "script-greek".to_string(),
            true,
            String::new()
        )]
    );
    // Nothing was fetched.
    assert!(shopping.requests().is_empty());
}

#[test]
fn download_fetches_the_pack_and_tells_the_one_who_asked() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.choose("Download");
    shopping.serve(PAGE, page().as_bytes());
    shopping.serve(
        "https://paperpaw.org/api/v1/plugins/script-greek/1.0.0/plugin.ini",
        MANIFEST.as_bytes(),
    );
    // Installed at once, and said to the one who asked, once, not as a
    // notice of the store's.
    assert_eq!(
        shopping
            .kernel
            .plugins()
            .get("script-greek")
            .map(|p| p.manifest.version),
        Some(Version::new(1, 0, 0))
    );
    assert_eq!(
        told(&answers),
        [(
            "script-greek".to_string(),
            String::new(),
            false,
            String::new()
        )]
    );
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/request.txt")
    );
    let shown = shopping.shown();
    assert!(!shown.contains("start the next time"), "{shown}");
}

#[test]
fn a_pack_that_cannot_be_fetched_is_told_with_the_reason() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.choose("Download");
    let call = shopping.request(PAGE);
    shopping.answer(&call, 500, b"");
    assert_eq!(
        told(&answers),
        [(
            String::new(),
            "script-greek".to_string(),
            false,
            "Could not download: Greek letters.".to_string()
        )]
    );
    assert!(shopping.kernel.plugins().get("script-greek").is_none());
}

#[test]
fn without_a_connection_the_one_who_asked_is_told_so_and_no_dialog_opens() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), false);
    shopping.ask("script-greek");
    let told = told(&answers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].1, "script-greek");
    assert!(!told[0].2, "{told:?}");
    assert!(told[0].3.starts_with("No connection."), "{told:?}");
    let shown = shopping.shown();
    assert!(!shown.contains("Download"), "{shown}");
}

/// What the library does not offer is not asked about and not an error:
/// it is told as missing, quietly.
#[test]
fn a_plugin_the_library_does_not_have_is_missing_quietly() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-korean");
    assert_eq!(
        told(&answers),
        [(
            String::new(),
            "script-korean".to_string(),
            false,
            String::new()
        )]
    );
    let shown = shopping.shown();
    assert!(!shown.contains("Download"), "{shown}");
}

/// What a plugin sends itself, past the kernel's checks, is cut down as
/// the kernel does: each id once, none already on the device, at most 32.
#[test]
fn what_is_asked_is_taken_once_without_what_is_there_and_at_most_32() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    // The store is on the device; the Greek letters are asked for twice.
    shopping.ask("store script-greek script-greek");
    let shown = shopping.shown();
    assert_eq!(shown.matches("Greek letters").count(), 1, "{shown}");
    assert!(told(&answers).is_empty());
    shopping.choose("Not now");
    assert_eq!(told(&answers).len(), 1);

    // Only what is there: nothing is missing and no dialog opens.
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("store");
    assert_eq!(
        told(&answers),
        [(String::new(), String::new(), false, String::new())]
    );
    assert!(!shopping.shown().contains("Download"));

    // 40 plugins the library lacks, then one it has: the cap cuts the
    // list at 32, the Greek letters (the 41st) are not asked for.
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    let many: Vec<String> = (0..40).map(|n| format!("none-{n}")).collect();
    shopping.ask(&format!("{} script-greek", many.join(" ")));
    let told = told(&answers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].1.split(' ').count(), 32, "{told:?}");
    assert!(!told[0].1.contains("script-greek"), "{told:?}");
    assert!(!shopping.shown().contains("Download"));
}

/// The same request again while its dialog is open is the one request;
/// one the reader just declined is told *Not now* at once, without a
/// dialog, until a minute has passed.
#[test]
fn the_same_request_again_is_one_and_a_declined_one_waits_a_minute() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.ask("script-greek");
    shopping.ask("script-greek");
    shopping.choose("Not now");
    // One dialog, one answer.
    assert_eq!(told(&answers).len(), 1);
    // Asked again at once: told at once, no dialog.
    shopping.ask("script-greek");
    let again = told(&answers);
    assert_eq!(again.len(), 2, "{again:?}");
    assert!(again[1].2, "declined: {again:?}");
    assert!(!shopping.shown().contains("Download"));
    // A minute later the reader is asked again.
    shopping.wait(61_000);
    shopping.ask("script-greek");
    assert!(shopping.shown().contains("Download"));
    assert_eq!(told(&answers).len(), 2);
}

/// What the reader sees of the download (#147): a screen with the names
/// and how far it is, which closes itself when the pack is installed.
#[test]
fn download_shows_how_far_it_is_and_closes_when_the_pack_is_installed() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.choose("Download");
    let shown = shopping.shown();
    assert!(shown.contains("Downloading"), "{shown}");
    assert!(shown.contains("Greek letters"), "{shown}");
    assert!(shown.contains("0 % · 0 B of 2.8 MB"), "{shown}");
    assert!(shown.contains("download goes on"), "{shown}");
    // The page and the file came: installed, the screen goes.
    shopping.serve(PAGE, page().as_bytes());
    shopping.serve(
        "https://paperpaw.org/api/v1/plugins/script-greek/1.0.0/plugin.ini",
        MANIFEST.as_bytes(),
    );
    let shown = shopping.shown();
    assert!(!shown.contains("Downloading"), "{shown}");
    assert_eq!(told(&answers).len(), 1);
}

/// What did not come is said, and *Try again* fetches it, without a new
/// dialog; the one who asked is told of each end.
#[test]
fn a_download_that_failed_offers_to_try_again() {
    let (mut shopping, answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.choose("Download");
    let call = shopping.request(PAGE);
    shopping.answer(&call, 500, b"");
    let shown = shopping.shown();
    assert!(
        shown.contains("Could not download: Greek letters."),
        "{shown}"
    );
    assert!(
        shown.contains("Try again") && shown.contains("Close"),
        "{shown}"
    );
    shopping.choose("Try again");
    let shown = shopping.shown();
    assert!(shown.contains("0 % · 0 B of 2.8 MB"), "{shown}");
    shopping.serve(PAGE, page().as_bytes());
    shopping.serve(
        "https://paperpaw.org/api/v1/plugins/script-greek/1.0.0/plugin.ini",
        MANIFEST.as_bytes(),
    );
    let shown = shopping.shown();
    assert!(!shown.contains("Downloading"), "{shown}");
    let told = told(&answers);
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(told[1].0, "script-greek", "{told:?}");
    assert!(shopping.kernel.plugins().get("script-greek").is_some());
}

/// *Close* after a failure leaves the screen; the book keeps what it has.
#[test]
fn a_failed_download_can_be_closed() {
    let (mut shopping, _answers) = Shopping::asked_for(&list(), true);
    shopping.ask("script-greek");
    shopping.choose("Download");
    let call = shopping.request(PAGE);
    shopping.answer(&call, 500, b"");
    shopping.choose("Close");
    let shown = shopping.shown();
    assert!(!shown.contains("Try again"), "{shown}");
    assert!(!shown.contains("Downloading"), "{shown}");
}

/// A download that is going on shows how many bytes are here: the files
/// in its folder against the size the library lists.
#[test]
fn the_bar_counts_the_bytes_of_the_files_that_are_here() {
    let (mut shopping, _answers) = Shopping::asked_for(&list(), true);
    // The Greek letters half downloaded (the job survives only after the
    // start, which begins a download cut off by a restart anew).
    let storage = &shopping.storage;
    storage.create_dir(".paperpaw/plugins/store").unwrap();
    storage
        .write(
            ".paperpaw/plugins/store/install.txt",
            format!(
                "1\tscript-greek\tGreek letters\t2\nplugin.ini\t90\t{}\thttps://x/a\nglyphs.ppfp\t2899910\t{}\thttps://x/b\n",
                "sha1:".to_string() + &"a".repeat(40),
                "sha256:".to_string() + &"b".repeat(64)
            )
            .as_bytes(),
        )
        .unwrap();
    storage
        .write(".paperpaw/plugins/store/install.done", b"1")
        .unwrap();
    storage.create_dir("plugins/.script-greek.part").unwrap();
    storage
        .write(
            "plugins/.script-greek.part/glyphs.ppfp",
            &vec![0u8; 1_450_000],
        )
        .unwrap();
    shopping.ask("script-greek");
    shopping.choose("Download");
    let shown = shopping.shown();
    assert!(
        shown.contains("50 % ·") && shown.contains("MB of 2.8 MB"),
        "{shown}"
    );
}
