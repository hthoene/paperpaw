//! The books Calibre sent, so it knows them when it connects again: each
//! one's metadata as Calibre gave it (without the cover thumbnail), in a
//! file of its own in the plugin's folder; what Calibre said about itself
//! (`SET_CALIBRE_DEVICE_INFO`); and the computer the reader chose to
//! trust (`host.txt`, its address on a line).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::digest::md5::Md5;
use paperpaw_sdk::{Env, Storage};

use crate::PLUGIN;
use crate::protocol;

/// Metadata kept at most per book (Calibre's descriptions can be long):
/// what fits in a message on the smallest devices.
const MAX_METADATA: usize = 64 * 1024;

fn folder() -> String {
    format!("{}/books", Env::plugin_dir(PLUGIN.as_str()))
}

fn device_file() -> String {
    format!("{}/device.json", Env::plugin_dir(PLUGIN.as_str()))
}

fn host_file() -> String {
    format!("{}/host.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The file of the book at `lpath`.
fn record(lpath: &str) -> String {
    let mut md5 = Md5::new();
    md5.update(lpath.as_bytes());
    format!("{}/{}.json", folder(), md5.hex())
}

/// The file of the book with `id` (see [`books`]).
fn record_of(id: u128) -> String {
    format!("{}/{id:032x}.json", folder())
}

/// Keeps the metadata of the book at `lpath`, without the cover
/// thumbnail.
pub fn remember(storage: &dyn Storage, lpath: &str, metadata: &str) {
    let metadata = protocol::without_member(metadata, "thumbnail");
    if metadata.len() > MAX_METADATA {
        return;
    }
    let _ = storage.create_dir(&folder());
    let _ = storage.write(&record(lpath), metadata.as_bytes());
}

/// Whether Calibre sent the book at `lpath` (and did not delete it
/// since): only such a book is replaced or deleted when Calibre asks.
pub fn sent(storage: &dyn Storage, lpath: &str) -> bool {
    storage.exists(&record(lpath))
}

/// Forgets the book at `lpath`; its `uuid`, if Calibre gave one.
pub fn forget(storage: &dyn Storage, lpath: &str) -> Option<String> {
    let path = record(lpath);
    let uuid = read(storage, &path).and_then(|m| protocol::text(&m, "uuid"));
    let _ = storage.remove(&path);
    uuid
}

fn read(storage: &dyn Storage, path: &str) -> Option<String> {
    let data = paperpaw_sdk::read_file(storage, path, MAX_METADATA).ok()?;
    String::from_utf8(data).ok()
}

/// The books Calibre sent that are still in `library` (the library
/// folder), by ids of 16 bytes each (their files' names), so a long list
/// takes little memory; their metadata one at a time with [`metadata`].
pub fn books(storage: &dyn Storage, library: &str) -> Vec<u128> {
    let mut ids = Vec::new();
    let _ = storage.list(&folder(), &mut |entry| {
        let id = entry
            .name
            .strip_suffix(".json")
            .filter(|hex| hex.len() == 32)
            .and_then(|hex| u128::from_str_radix(hex, 16).ok());
        if let (false, Some(id)) = (entry.is_dir, id) {
            ids.push(id);
        }
    });
    paperpaw_sdk::sort::sort(&mut ids);
    ids.retain(|id| {
        metadata(storage, *id)
            .and_then(|metadata| protocol::text(&metadata, "lpath"))
            .and_then(|lpath| protocol::safe_path(&lpath))
            .is_some_and(|lpath| storage.exists(&paperpaw_sdk::files::join(library, &lpath)))
    });
    ids
}

/// The metadata of the book with `id`, as Calibre gave it.
pub fn metadata(storage: &dyn Storage, id: u128) -> Option<String> {
    read(storage, &record_of(id))
}

/// What Calibre said about itself last.
pub fn device_info(storage: &dyn Storage) -> Option<String> {
    read(storage, &device_file())
}

pub fn set_device_info(storage: &dyn Storage, info: &str) {
    let _ = storage.create_dir(Env::plugin_dir(PLUGIN.as_str()).as_str());
    let _ = storage.write(&device_file(), info.as_bytes());
}

/// The address of the computer the reader chose to trust, if any.
pub fn trusted(storage: &dyn Storage) -> Option<String> {
    let data = paperpaw_sdk::read_file(storage, &host_file(), 256).ok()?;
    let text = core::str::from_utf8(&data).ok()?;
    let host = text.lines().next()?.trim();
    (!host.is_empty()).then(|| host.to_string())
}

/// Trusts the computer at `host` from now on, in place of another.
pub fn trust(storage: &dyn Storage, host: &str) {
    let _ = storage.create_dir(Env::plugin_dir(PLUGIN.as_str()).as_str());
    let _ = storage.write(&host_file(), format!("{host}\n").as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn books_kept_while_their_files_are_there() {
        let storage = MemoryStorage::new().with("books/Ann/A.epub", b"x".to_vec());
        let a = r#"{"lpath": "Ann/A.epub", "uuid": "u-a", "title": "A"}"#;
        let b = r#"{"lpath": "Bo/B.epub", "uuid": "u-b", "title": "B"}"#;
        remember(&storage, "Ann/A.epub", a);
        remember(&storage, "Bo/B.epub", b);
        assert!(sent(&storage, "Ann/A.epub") && sent(&storage, "Bo/B.epub"));
        assert!(!sent(&storage, "Ann/Other.epub"));
        // B's file is not there (deleted on the device).
        let listed = books(&storage, "books");
        assert_eq!(listed.len(), 1);
        assert_eq!(metadata(&storage, listed[0]).as_deref(), Some(a));
        assert_eq!(
            record_of(listed[0]),
            format!(
                "{}/{}.json",
                folder(),
                paperpaw_sdk::digest::md5::of("Ann/A.epub")
            )
        );
        assert_eq!(forget(&storage, "Ann/A.epub").as_deref(), Some("u-a"));
        assert!(books(&storage, "books").is_empty());
        assert!(!sent(&storage, "Ann/A.epub"));
        assert_eq!(forget(&storage, "Ann/A.epub"), None);
    }

    #[test]
    fn thumbnails_are_not_kept() {
        let storage = MemoryStorage::new();
        let metadata = r#"{"lpath": "A.epub", "thumbnail": [1, 2, "AAAA"], "title": "A"}"#;
        remember(&storage, "A.epub", metadata);
        let kept = read(&storage, &record("A.epub")).unwrap();
        assert_eq!(kept, r#"{"lpath": "A.epub", "title": "A"}"#);
    }

    #[test]
    fn the_trusted_computer_is_kept() {
        let storage = MemoryStorage::new();
        assert_eq!(trusted(&storage), None);
        trust(&storage, "192.168.1.5");
        assert_eq!(trusted(&storage).as_deref(), Some("192.168.1.5"));
        trust(&storage, "10.0.0.2");
        assert_eq!(trusted(&storage).as_deref(), Some("10.0.0.2"));
        // As the first version wrote it.
        let v1 = include_bytes!("../testdata/host.txt.v1");
        storage.write(&host_file(), v1).unwrap();
        assert_eq!(trusted(&storage).as_deref(), Some("192.168.1.20"));
    }

    #[test]
    fn device_info_is_kept() {
        let storage = MemoryStorage::new();
        assert_eq!(device_info(&storage), None);
        set_device_info(&storage, r#"{"device_store_uuid": "x"}"#);
        assert_eq!(
            device_info(&storage).as_deref(),
            Some(r#"{"device_store_uuid": "x"}"#)
        );
    }
}
