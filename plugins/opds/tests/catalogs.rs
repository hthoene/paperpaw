//! The list of catalogs and a catalog's details with the real kernel:
//! adding one by its address, editing its fields as the keyboard answers,
//! switching sync and WebDAV, removing one; `catalogs.txt` written and
//! read back, odd lines in it skipped, a list that is no text left as it
//! is, and passwords kept with the other
//! secrets, never in the list.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "harness/mod.rs"]
mod common;

use common::{CATALOGS, Harness, Shared, catalogs};
use paperpaw_plugin_opds::OPEN;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{Id, Instant, Params};

fn typed(text: &str) -> Typed {
    Typed::new(String::from(text), Params::new())
}

/// What the keyboard answers for a field of the details.
fn field(key: &'static str, text: &str) -> Typed {
    Typed::new(String::from(text), Params::new().with("field", key))
}

fn picked(index: &str, option: &str) -> Picked {
    Picked::new(String::from(index), String::from(option))
}

/// The lines of the list, each split into its fields.
fn lines(h: &Harness) -> Vec<Vec<String>> {
    h.storage
        .text(CATALOGS)
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('\t').map(String::from).collect())
        .collect()
}

fn secret_id(url: &str) -> String {
    format!("opds.{}", &paperpaw_sdk::digest::md5::of(url)[..16])
}

#[test]
fn a_catalog_added_by_its_address_is_kept_and_edited() {
    let mut h = Harness::new(Shared::default());
    h.run(OPEN);
    // The start catalog, then *Add catalog*.
    h.run(actions::DOWN);
    h.run(actions::CONFIRM);
    assert!(h.requests().is_empty());
    h.send("opds.address", typed("  calibre.local:8080/opds  "));
    let saved = lines(&h);
    assert_eq!(saved.len(), 2, "{saved:?}");
    assert_eq!(saved[1][0], "calibre.local:8080");
    assert_eq!(saved[1][1], "https://calibre.local:8080/opds");
    // The details are open now.
    assert!(h.shows("User name"));
    h.send("opds.entered", field("opds.field.user", "  reader  "));
    h.send(
        "opds.entered",
        field("opds.field.name", "My\tCalibre\nat home"),
    );
    h.send("opds.entered", field("opds.field.name", "   "));
    h.send(
        "opds.entered",
        field("opds.field.folder", "/Books/Calibre/"),
    );
    h.send("opds.entered", field("opds.field.address", ""));
    h.send("opds.entered", field("opds.field.unknown", "ignored"));
    let saved = lines(&h);
    assert_eq!(saved.len(), 2, "one line a catalog: {saved:?}");
    assert_eq!(saved[1].len(), 7, "{saved:?}");
    assert_eq!(saved[1][0], "My Calibre at home");
    assert_eq!(saved[1][1], "https://calibre.local:8080/opds");
    assert_eq!(saved[1][2], "reader");
    assert_eq!(saved[1][4], "Books/Calibre");
    // Sync is the last field, WebDAV the third.
    for _ in 0..6 {
        h.run(actions::DOWN);
    }
    h.run(actions::CONFIRM);
    assert_eq!(lines(&h)[1][5], "sync");
    for _ in 0..4 {
        h.run(actions::UP);
    }
    h.run(actions::CONFIRM);
    assert_eq!(lines(&h)[1][6], "webdav");
    h.run(actions::CONFIRM);
    assert_eq!(lines(&h)[1][6], "");
    // Back in the list, as named.
    h.run(actions::BACK);
    assert!(h.shows("My Calibre at home"));
    assert!(h.said().contains("synced to Books/Calibre"));
}

#[test]
fn a_password_typed_is_a_secret_not_a_line_in_the_list() {
    let mut h = Harness::new(Shared::with(&[(
        CATALOGS,
        &catalogs(&[["Calibre", "https://c.example/opds", "me", "", "", "", ""]]),
    )]));
    h.kernel.open_screen(
        Instant(1),
        Id::new_static("opds.edit"),
        Params::new().with("id", "0"),
    );
    // Passwords keep their spaces.
    h.send("opds.entered", field("opds.field.password", " pass word "));
    let id = secret_id("https://c.example/opds");
    assert_eq!(h.kernel.secrets().get(&id), Some(" pass word "));
    let list = h.storage.text(CATALOGS).unwrap();
    assert!(!list.contains("pass word"), "{list}");
    let file = h.storage.text(FILE).unwrap();
    assert!(!file.contains("pass word"), "{file}");
    assert_eq!(Secrets::decode(&file).get(&id), Some(" pass word "));
    // It shows as dots.
    assert!(!h.said().contains("pass word"));
    // Emptied, it goes.
    h.send("opds.entered", field("opds.field.password", ""));
    assert_eq!(h.kernel.secrets().get(&id), None);
}

#[test]
fn a_new_address_moves_the_password_with_it() {
    let old = "https://old.example/opds";
    let mut h = Harness::new(Shared::with(&[(
        CATALOGS,
        &catalogs(&[["Mine", old, "me", "", "", "", ""]]),
    )]));
    h.kernel
        .set_secret(SecretChange::keep(&secret_id(old), "x", "pw"));
    h.kernel.open_screen(
        Instant(1),
        Id::new_static("opds.edit"),
        Params::new().with("id", "0"),
    );
    // The address changes: the password goes with it, none is left behind.
    h.send(
        "opds.entered",
        field("opds.field.address", "https://new.example/opds"),
    );
    assert_eq!(h.kernel.secrets().get(&secret_id(old)), None, "no orphan");
    assert_eq!(
        h.kernel
            .secrets()
            .get(&secret_id("https://new.example/opds")),
        Some("pw")
    );
}

#[test]
fn removing_a_catalog_removes_its_password() {
    let url = "https://c.example/opds";
    let mut h = Harness::new(Shared::with(&[(
        CATALOGS,
        &catalogs(&[
            [
                "Gutenberg",
                "https://www.gutenberg.org/ebooks.opds/",
                "",
                "",
                "",
                "",
                "",
            ],
            ["Calibre", url, "me", "", "", "", ""],
        ]),
    )]));
    h.kernel
        .set_secret(SecretChange::keep(&secret_id(url), "x", "pw"));
    h.run(OPEN);
    // Choices for rows that are not there, or not numbers, do nothing.
    for (index, option) in [
        ("7", "remove"),
        ("-1", "remove"),
        ("x", "edit"),
        ("1", "dance"),
    ] {
        h.send("opds.chosen", picked(index, option));
    }
    assert_eq!(lines(&h).len(), 2);
    h.send("opds.chosen", picked("1", "remove"));
    let saved = lines(&h);
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(saved[0][0], "Gutenberg");
    assert_eq!(h.kernel.secrets().get(&secret_id(url)), None);
    assert!(!h.shows("Calibre"));
    // The last one too: the list is empty then, not the start catalog again.
    h.send("opds.chosen", picked("0", "remove"));
    assert_eq!(h.storage.text(CATALOGS).as_deref(), Some(""));
    h.run(actions::BACK);
    h.run(OPEN);
    assert!(!h.shows("Gutenberg"));
    assert!(h.shows("Add catalog"));
}

#[test]
fn addresses_that_are_none_are_not_added() {
    let mut h = Harness::new(Shared::default());
    h.run(OPEN);
    for text in ["", "   ", "https://"] {
        h.send("opds.address", typed(text));
    }
    assert!(h.storage.text(CATALOGS).is_none());
    // Ones with their own scheme keep it.
    h.send("opds.address", typed("http://192.168.1.2/opds"));
    assert_eq!(lines(&h)[1][1], "http://192.168.1.2/opds");
    // Nextcloud's addresses are WebDAV folders.
    h.run(actions::BACK);
    h.send(
        "opds.address",
        typed("cloud.example/remote.php/dav/files/me/"),
    );
    assert_eq!(lines(&h)[2][6], "webdav");
}

#[test]
fn odd_lines_in_the_list_are_skipped() {
    let mut text = Vec::new();
    text.extend_from_slice(b"Good one\thttps://good.example/opds\r\n");
    text.extend_from_slice(b"No address\t\tuser\n");
    text.extend_from_slice(b"Only a name\n");
    text.extend_from_slice(b"\n\t\t\t\t\t\t\n");
    text.extend_from_slice("Bücher \u{1f4da}\thttps://unicode.example/\n".as_bytes());
    text.extend_from_slice(b"Extra\thttps://extra.example/\tu\t\tf\tsync\twebdav\tmore\tfields\n");
    let mut h = Harness::new(Shared::with(&[(CATALOGS, &text)]));
    h.run(OPEN);
    let shown = h.said();
    assert!(shown.contains("Good one"), "{shown}");
    assert!(shown.contains("Bücher"), "{shown}");
    assert!(shown.contains("Extra"), "{shown}");
    assert!(!shown.contains("No address"), "{shown}");
    assert!(!shown.contains("Only a name"), "{shown}");
    // A marked catalog shows *Sync catalogs* first.
    assert!(shown.starts_with("Catalogs Sync catalogs"), "{shown}");
}

/// A list that is no UTF-8 text (saved in another encoding, or damaged)
/// is not read, says so, and is never saved over: its catalogs would be
/// lost.
#[test]
fn a_list_that_cannot_be_read_says_so_and_is_kept() {
    let text = b"Good one\thttps://good.example/opds\nBad \xff\xfe bytes\thttps://bad.example/\n";
    let mut h = Harness::new(Shared::with(&[(CATALOGS, text)]));
    h.run(OPEN);
    let shown = h.said();
    assert!(shown.contains("could not be read"), "{shown}");
    assert!(!shown.contains("Good one"), "{shown}");
    h.send("opds.address", typed("https://new.example/opds"));
    assert_eq!(h.storage.0.get(CATALOGS).unwrap(), text);
}

#[test]
fn details_of_a_catalog_that_is_not_there_change_nothing() {
    let list = catalogs(&[["Mine", "https://m.example/opds", "", "", "", "", ""]]);
    let mut h = Harness::new(Shared::with(&[(CATALOGS, &list)]));
    for id in ["5", "-1", "nonsense", ""] {
        h.kernel.open_screen(
            Instant(1),
            Id::new_static("opds.edit"),
            Params::new().with("id", id),
        );
        h.send("opds.entered", field("opds.field.name", "Changed"));
        h.send("opds.entered", field("opds.field.password", "pw"));
        h.run(actions::BACK);
    }
    assert_eq!(h.storage.0.get(CATALOGS).unwrap(), list);
    assert!(
        h.kernel
            .secrets()
            .iter()
            .all(|s| !s.id.starts_with("opds."))
    );
}
