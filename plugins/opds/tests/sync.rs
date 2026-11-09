//! Syncing a catalog with the real kernel: its new books are downloaded
//! into its folder, a book synced before is not fetched again however
//! long the record of them grows, a record that cannot be read leaves the
//! catalog alone, and stopping leaves no file behind.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Device, Shared, catalogs_path, plugin_dir};
use paperpaw_core::HttpCall;
use paperpaw_plugin_opds::{OPEN, SYNC};
use paperpaw_sdk::Storage;
use paperpaw_sdk::builtin::actions;

const SHELF: &str = "https://books.example/shelf";

const FEED: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom">
<title>Shelf</title>
<entry>
  <id>urn:book:1</id>
  <title>One</title>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/one.epub"/>
</entry>
</feed>"#;

/// Where the book goes.
const BOOK: &str = "Shelf/One.epub";

/// The books synced from the shelf before.
fn record_path() -> String {
    format!(
        "{}/synced/{}.txt",
        plugin_dir(),
        paperpaw_sdk::digest::md5::of(SHELF)
    )
}

/// A storage with the shelf, marked for syncing.
fn shelf() -> Shared {
    let storage = Shared::default();
    let line = format!("Shelf\t{SHELF}\t\t\t\tsync\t\n");
    storage.write(&catalogs_path(), line.as_bytes()).unwrap();
    storage
}

/// Syncs: the shelf's list is asked for and answered; then the requests
/// that follow.
fn sync(device: &mut Device) -> Vec<HttpCall> {
    device.run(SYNC);
    let call = device.request();
    assert_eq!(call.url, SHELF);
    device.answer(&call, 200, FEED);
    device.requests()
}

#[test]
fn a_deleted_book_is_not_fetched_again() {
    let storage = shelf();
    let mut device = Device::start(storage.clone());
    let calls = sync(&mut device);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].url, "https://books.example/one.epub");
    device.answer(&calls[0], 200, "PK one");
    assert_eq!(storage.get(BOOK).as_deref(), Some(&b"PK one"[..]));
    assert_eq!(storage.get(&record_path()).unwrap(), b"urn:book:1\n");
    storage.remove(BOOK).unwrap();
    assert!(sync(&mut device).is_empty());
}

#[test]
fn a_long_record_still_counts() {
    let storage = shelf();
    // More than a megabyte of books synced before, the one listed last.
    let mut record = "urn:book:old\n".repeat(100_000);
    record.push_str("urn:book:1\n");
    storage.write(&record_path(), record.as_bytes()).unwrap();
    let mut device = Device::start(storage);
    let calls = sync(&mut device);
    assert!(calls.is_empty(), "{calls:?}");
}

#[test]
fn a_record_that_cannot_be_read_leaves_the_catalog_alone() {
    let storage = shelf();
    storage.write(&record_path(), b"urn:book:1\n").unwrap();
    storage.break_file(&record_path());
    let mut device = Device::start(storage.clone());
    let calls = sync(&mut device);
    assert!(calls.is_empty(), "{calls:?}");
    assert!(!storage.exists(BOOK));
    assert_eq!(storage.get(&record_path()).unwrap(), b"urn:book:1\n");
}

#[test]
fn stopping_leaves_no_file_behind() {
    let storage = shelf();
    let mut device = Device::start(storage.clone());
    let calls = sync(&mut device);
    assert_eq!(calls.len(), 1, "{calls:?}");
    // *Stop syncing* heads the list while it runs.
    device.run(OPEN);
    device.run(actions::CONFIRM);
    device.answer(&calls[0], 200, "PK one");
    assert!(!storage.exists(calls[0].save_to.as_deref().unwrap()));
    assert!(!storage.exists(BOOK));
}

#[test]
fn what_was_left_goes_at_start() {
    let storage = Shared::default();
    let left = [
        format!("{}/sync.tmp", plugin_dir()),
        format!("{}/tmp/7.tmp", plugin_dir()),
    ];
    for path in &left {
        storage.write(path, b"half a book").unwrap();
    }
    let _device = Device::start(storage.clone());
    for path in &left {
        assert!(!storage.exists(path), "{path}");
    }
}
