//! Code plugins in the store with the real kernel on an X4: listed where a
//! build fits this PaperPaw, said once what they are before the first,
//! installed with their code checked whole and signed by their author
//! (remembered from then on, also after a removal), starting after a
//! restart; an update or a new install signed by another key is refused;
//! code that is damaged,
//! unsigned, made for another build or not by the author the library
//! names is not installed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{LIST, Shared, Shopping};
use paperpaw_native::signature::{public_key, sign};
use paperpaw_native::{Chip, Header, LINK_BASE};
use paperpaw_plugin_store::OPEN;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::native::{Fingerprint, Native, installed_code, known_author};
use paperpaw_sdk::{PluginKind, Storage, Version};

const PAGE: &str = "https://paperpaw.org/api/v1/plugins/clock.json?device=x4&lang=en";
const FILES: &str = "https://paperpaw.org/api/v1/plugins/clock";
/// The build of this PaperPaw.
const BUILD: Fingerprint = Fingerprint(0x0123_4567_89ab_cdef);
/// The seeds of the keys of the plugin's author and of someone else.
const AUTHOR: [u8; 32] = [7; 32];
const OTHER: [u8; 32] = [9; 32];

fn native() -> Native {
    let mut native = Native::new("code.ppn");
    native.fingerprint = BUILD;
    native
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A plugin file built for `build`, signed with the key of `seed` (or not
/// at all).
fn ppn(build: Fingerprint, seed: Option<&[u8; 32]>) -> Vec<u8> {
    let image = [0u8; 128];
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: build.0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: paperpaw_sdk::native::INTERFACE,
        sha256: [0; 32],
    };
    let mut sha = header.digest();
    sha.update(&image);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    if let Some(seed) = seed {
        let signature = sign(&file, seed);
        file.extend_from_slice(&signature.to_bytes());
    }
    file
}

fn manifest(version: &str) -> String {
    format!("[plugin]\nid = clock\nname = Clock\nversion = {version}\n")
}

/// The list: the clock in `version`, by the author.
fn list(version: &str) -> String {
    format!(
        r#"{{"format": 2, "plugins": [
  {{"id": "clock", "name": "Clock", "summary": "A clock on the home screen.",
    "group": "reading", "author": "Someone", "licence": "MIT", "kind": "code",
    "author_key": "{}",
    "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": 40,
      "builds": [{{"device": "x4", "abi": "{BUILD}", "url": "clock/{version}/x4.ppn",
                   "size": 1, "sha256": "{}"}}]}}}}]}}"#,
        hex(&public_key(&AUTHOR)),
        "0".repeat(64),
    )
}

/// The clock's page at the library: its plugin.ini and `code`.
fn page(version: &str, code: &[u8]) -> String {
    let ini = manifest(version);
    format!(
        r#"{{"format": 2, "plugin": {{"id": "clock", "name": "Clock",
  "summary": "A clock on the home screen.", "group": "reading",
  "author": "Someone", "licence": "MIT", "kind": "code", "author_key": "{}",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": {},
    "files": [{{"path": "plugin.ini", "url": "clock/{version}/plugin.ini",
                "size": {}, "sha1": "{}"}}],
    "builds": [{{"device": "x4-pro", "abi": "{BUILD}", "url": "clock/{version}/x4-pro.ppn",
                 "size": 1, "sha256": "{}"}},
               {{"device": "x4", "abi": "{BUILD}", "url": "clock/{version}/x4.ppn",
                 "size": {}, "sha256": "{}"}}]}}}}}}"#,
        hex(&public_key(&AUTHOR)),
        ini.len(),
        ini.len(),
        paperpaw_digest::sha1::hex(ini.as_bytes()),
        "0".repeat(64),
        code.len(),
        paperpaw_digest::sha256::hex(code),
    )
}

/// The store on an X4 that loads code plugins of [`BUILD`].
fn store(version: &str) -> Shopping {
    Shopping::on(Some("x4"), Some(native()), &list(version))
}

/// The clock's page opened from its group's list.
fn open_clock(shopping: &mut Shopping, version: &str, code: &[u8]) {
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, page(version, code).as_bytes());
}

/// The clock's files answered: its plugin.ini, then `code`.
fn download(shopping: &mut Shopping, version: &str, code: &[u8]) {
    let ini = manifest(version);
    shopping.serve(&format!("{FILES}/{version}/plugin.ini"), ini.as_bytes());
    shopping.serve(&format!("{FILES}/{version}/x4.ppn"), code);
}

/// The clock installed the first time: its page, the notice, its files.
fn first_install(shopping: &mut Shopping, version: &str, code: &[u8]) {
    open_clock(shopping, version, code);
    let shown = shopping.shown();
    assert!(
        shown.contains("Code plugin for PaperPaw 0.4: runs with full access to this device."),
        "{shown}"
    );
    // *Install*: first what code plugins are.
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    assert!(shown.contains("Code plugins are programs"), "{shown}");
    assert!(shown.contains("I understand, install | Not now"), "{shown}");
    shopping.run(actions::CONFIRM);
    download(shopping, version, code);
}

fn installed(shopping: &Shopping) -> Option<Version> {
    shopping
        .kernel
        .plugins()
        .get("clock")
        .map(|p| p.update.unwrap_or(p.manifest.version))
}

#[test]
fn a_code_plugin_is_installed_and_starts_after_a_restart() {
    let mut shopping = store("1.0.0");
    let code = ppn(BUILD, Some(&AUTHOR));
    // Its page, and what code plugins are, said before the first.
    open_clock(&mut shopping, "1.0.0", &code);
    shopping.check("code-page");
    shopping.run(actions::CONFIRM);
    shopping.check("code-notice");
    // *Back*: the plugin again, nothing installed.
    shopping.run(actions::BACK);
    let shown = shopping.shown();
    assert!(shown.contains("Install, 405 B"), "{shown}");
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.0.0").as_bytes());
    first_install(&mut shopping, "1.0.0", &code);
    let clock = shopping.kernel.plugins().get("clock").unwrap();
    assert_eq!(clock.kind, PluginKind::Code);
    assert!(clock.restart_needed());
    assert!(shopping.storage.exists("plugins/clock/code.ppn"));
    // Its author remembered for its updates.
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
    // Its code named as installed: only that file runs.
    assert_eq!(installed_code(storage, "clock"), Some(digest(&code)));
    let shown = shopping.shown();
    assert!(shown.contains("Restart now"), "{shown}");
    assert!(
        shown.contains("The change applies once PaperPaw starts again."),
        "{shown}"
    );
}

#[test]
fn an_update_by_another_key_is_refused() {
    let mut shopping = store("1.0.0");
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    // A newer version, its code signed by someone else.
    let other = ppn(BUILD, Some(&OTHER));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.1.0").as_bytes());
    open_clock(&mut shopping, "1.1.0", &other);
    let shown = shopping.shown();
    assert!(
        shown.contains("Restart now | Update, 1.1.0 | Remove"),
        "{shown}"
    );
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.1.0", &other);
    // Refused at once, with a short warning: no way to install it anyway.
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("signed by another key than before"),
        "{shown}"
    );
    assert!(!shown.contains("anyway"), "{shown}");
    assert!(!shopping.storage.exists("plugins/.clock.part/code.ppn"));
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
}

#[test]
fn a_new_install_after_a_removal_keeps_the_first_key() {
    // A device that had the clock, signed by its author, and removed it:
    // the key stays known.
    let storage = Shared::default();
    assert!(paperpaw_sdk::native::remember_author(
        &storage,
        "clock",
        &public_key(&AUTHOR)
    ));
    // The library now names someone else's key for it.
    let theirs = list("1.0.0").replace(&hex(&public_key(&AUTHOR)), &hex(&public_key(&OTHER)));
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &theirs, storage);
    let code = ppn(BUILD, Some(&OTHER));
    shopping.choose("Reading");
    shopping.choose("Clock");
    let page = page("1.0.0", &code).replace(&hex(&public_key(&AUTHOR)), &hex(&public_key(&OTHER)));
    shopping.serve(PAGE, page.as_bytes());
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.0.0", &code);
    assert_eq!(installed(&shopping), None);
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("signed by another key than before"),
        "{shown}"
    );
    assert_eq!(
        known_author(&shopping.storage, "clock"),
        Some(public_key(&AUTHOR))
    );
}

/// The SHA-256 a plugin file's header carries.
fn digest(file: &[u8]) -> [u8; 32] {
    Header::read(file).unwrap().sha256
}

/// The clock's first install with `code`: not installed, the reader told
/// `why`.
fn refused(code: &[u8], why: &str) {
    let mut shopping = store("1.0.0");
    first_install(&mut shopping, "1.0.0", code);
    assert_eq!(installed(&shopping), None, "{why}");
    assert!(!shopping.storage.exists("plugins/clock/code.ppn"), "{why}");
    assert!(
        !shopping.storage.exists("plugins/.clock.part/code.ppn"),
        "{why}"
    );
    assert_eq!(known_author(&shopping.storage, "clock"), None, "{why}");
    assert_eq!(installed_code(&shopping.storage, "clock"), None, "{why}");
    // Said in the notice over the page, its lines joined.
    let shown = shopping.shown().replace(" | ", " ");
    assert!(shown.contains(why), "{why}: {shown}");
}

#[test]
fn code_that_cannot_be_trusted_is_not_installed() {
    refused(
        &ppn(BUILD, Some(&OTHER)),
        "not signed by the author the library names",
    );
    refused(&ppn(BUILD, None), "not signed by its author");
    refused(
        &ppn(Fingerprint(1), Some(&AUTHOR)),
        "built for another plugin interface or compiler",
    );
    let mut damaged = ppn(BUILD, Some(&AUTHOR));
    damaged[100] ^= 0xFF;
    refused(&damaged, "its code is damaged");
}

#[test]
fn without_code_plugins_none_is_listed() {
    let mut shopping = Shopping::on(Some("x4"), None, &list("1.0.0"));
    let shown = shopping.shown();
    assert!(!shown.contains("Clock"), "{shown}");
}

/// A storage where code plugins have `total` bytes, in pages of 4 KiB,
/// and another code plugin takes one page.
fn with_room(total: u64) -> Shared {
    with_room_for(total, 83)
}

/// As [`with_room`], for `most` code plugins at most.
fn with_room_for(total: u64, most: u32) -> Shared {
    let storage = Shared::default();
    let room = format!("{total} 4096 4096 {most} 1 0\n");
    storage
        .write(".paperpaw/code-room.txt", room.as_bytes())
        .unwrap();
    let other = ppn(BUILD, Some(&OTHER));
    storage.write("plugins/other/code.ppn", &other).unwrap();
    storage
}

#[test]
fn code_that_does_not_fit_is_not_offered() {
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), with_room(8192));
    shopping.choose("Reading");
    shopping.choose("Clock");
    // The library says its code takes two pages; one is left.
    let code = ppn(BUILD, Some(&AUTHOR));
    let body = page("1.0.0", &code).replace(
        r#""device": "x4", "abi""#,
        r#""device": "x4", "image_len": 8192, "abi""#,
    );
    shopping.serve(PAGE, body.as_bytes());
    let shown = shopping.shown();
    assert!(
        shown.contains("Needs 8.0 KB for its code, only 4.0 KB left."),
        "{shown}"
    );
    assert!(shown.contains("Not enough room"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
    // Where the library does not say, it is offered with what is left.
    shopping.run(actions::BACK);
    shopping.choose("Clock");
    shopping.serve(PAGE, page("1.0.0", &code).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("4.0 KB left for code plugins"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn code_that_does_not_fit_is_refused_once_here() {
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), with_room(4096));
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    assert_eq!(installed(&shopping), None);
    assert!(!shopping.storage.exists("plugins/clock/code.ppn"));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("not enough room left for code plugins"),
        "{shown}"
    );
}

#[test]
fn code_that_just_fits_is_installed() {
    // Room for exactly two pages and two plugins: the other one's and
    // the clock's. The clock's download, put together beside it, is not
    // counted as one more.
    let storage = with_room_for(8192, 2);
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), storage);
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    assert!(shopping.storage.exists("plugins/clock/code.ppn"));
    // Now the room is full: the Installed list says so.
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.0.0").as_bytes());
    shopping.choose("Installed");
    let shown = shopping.shown();
    assert!(
        shown.contains("code plugins: 8.0 KB of 8.0 KB used"),
        "{shown}"
    );
}

/// PaperPaw's own plugins say so on their row and their page; the list's
/// word alone, with another author's key, gives no such mark.
#[test]
fn paperpaws_own_plugins_say_so() {
    const OURS: &str = "148ccf3ebc30506b355c6b5cb2baf87d9265606cc6a81f085bb3b9740a95aebd";
    let author = hex(&public_key(&AUTHOR));
    let claimed = |text: String| {
        text.replace(
            r#""author": "Someone","#,
            r#""author": "Someone", "official": true,"#,
        )
    };
    let code = ppn(BUILD, Some(&AUTHOR));

    let mut shopping = Shopping::on(
        Some("x4"),
        Some(native()),
        &claimed(list("1.0.0")).replace(&author, OURS),
    );
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(
        shown.contains("By PaperPaw · A clock on the home screen."),
        "{shown}"
    );
    shopping.choose("Clock");
    let ours = claimed(page("1.0.0", &code)).replace(&author, OURS);
    shopping.serve(PAGE, ours.as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("By PaperPaw"), "{shown}");
    assert!(!shown.contains("By Someone"), "{shown}");

    // The same word with the author's own key: no mark.
    let mut shopping = Shopping::on(Some("x4"), Some(native()), &claimed(list("1.0.0")));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(!shown.contains("By PaperPaw"), "{shown}");
    shopping.choose("Clock");
    shopping.serve(PAGE, claimed(page("1.0.0", &code)).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("By Someone"), "{shown}");
    assert!(!shown.contains("By PaperPaw"), "{shown}");
}

/// `text` (the list or a page) as the library gives a community plugin,
/// which nobody reviewed.
fn unreviewed(text: &str) -> String {
    text.replace(
        r#""author": "Someone","#,
        r#""author": "Someone", "unreviewed": true,
           "repository": "https://github.com/someone/clock","#,
    )
}

#[test]
fn a_community_plugin_is_marked_and_its_key_may_not_change() {
    let mut shopping = Shopping::on(Some("x4"), Some(native()), &unreviewed(&list("1.0.0")));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(shown.contains("Unreviewed · A clock"), "{shown}");
    let code = ppn(BUILD, Some(&AUTHOR));
    shopping.choose("Clock");
    shopping.serve(PAGE, unreviewed(&page("1.0.0", &code)).as_bytes());
    let shown = shopping.shown();
    assert!(
        shown.contains("Unreviewed, from github.com/someone/clock"),
        "{shown}"
    );
    // Its key, which its updates must have too.
    let key = hex(&public_key(&AUTHOR));
    let short = format!(
        "Key {} {} {} {}",
        &key[..4],
        &key[4..8],
        &key[8..12],
        &key[12..16]
    );
    assert!(shown.contains(&short), "{shown}");
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.0.0", &code);
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));

    // An update signed by someone else: refused.
    let other = ppn(BUILD, Some(&OTHER));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, unreviewed(&list("1.1.0")).as_bytes());
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, unreviewed(&page("1.1.0", &other)).as_bytes());
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.1.0", &other);
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("signed by another key than before"),
        "{shown}"
    );
    assert!(!shown.contains("anyway"), "{shown}");
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
    assert!(!storage.exists("plugins/.clock.part/code.ppn"));
}
