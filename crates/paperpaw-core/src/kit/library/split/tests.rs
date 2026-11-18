#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::*;
use crate::kit::library::{self, BookData, BookDataKind, key};
use paperpaw_sdk::MemoryStorage;

/// Where the details of the book at `path` are kept now.
fn details(path: &str) -> String {
    let key = key(path);
    format!(".paperpaw/library/details/{}/{key}.ini", &key[..2])
}

/// Where earlier versions kept them.
fn old_details(path: &str) -> String {
    format!(".paperpaw/library/books/{}.ini", key(path))
}

/// Where earlier versions kept the cover of the book at `path` made
/// small, of `variant` (`.row` or none).
fn old_thumb(path: &str, variant: &str) -> String {
    format!(".paperpaw/library/covers/{}{variant}.thumb", key(path))
}

fn thumb(path: &str, variant: &str) -> String {
    let key = key(path);
    format!(
        ".paperpaw/library/thumbs/{}/{key}{variant}.thumb",
        &key[..2]
    )
}

/// Every step of `split` until it is done; how many it took.
fn finish(split: &mut Split, storage: &dyn Storage) -> usize {
    let mut steps = 0;
    while !split.step(storage, &[]).unwrap() {
        steps += 1;
        assert!(steps < 10_000, "the moving does not end");
    }
    steps
}

#[test]
fn everything_is_moved_into_the_parts_of_its_keys_and_the_old_folders_go() {
    let storage = MemoryStorage::new();
    let paths: Vec<String> = (0..700).map(|i| format!("books/{i}.epub")).collect();
    for (i, path) in paths.iter().enumerate() {
        storage
            .write(&old_details(path), format!("n={i}").as_bytes())
            .unwrap();
        if i % 3 == 0 {
            storage
                .write(&old_thumb(path, ""), format!("c{i}").as_bytes())
                .unwrap();
            storage
                .write(&old_thumb(path, ".row"), format!("r{i}").as_bytes())
                .unwrap();
        }
    }
    // Not a book's: taken away with the folder, the library's own.
    storage
        .write(".paperpaw/library/books/notes.txt", b"x")
        .unwrap();
    storage
        .write(".paperpaw/library/books/0123.ini", b"x")
        .unwrap();
    let mut split = Split::new();
    let steps = finish(&mut split, &storage);
    // Listed 256 at a time, moved 16 a step.
    assert!(steps >= 700 / 16, "{steps}");
    assert_eq!(split.moved(), 700 + 2 * 234);
    for (i, path) in paths.iter().enumerate() {
        assert_eq!(
            storage.get(&details(path)),
            Some(format!("n={i}").into_bytes())
        );
        if i % 3 == 0 {
            assert_eq!(
                storage.get(&thumb(path, "")),
                Some(format!("c{i}").into_bytes())
            );
            assert_eq!(
                storage.get(&thumb(path, ".row")),
                Some(format!("r{i}").into_bytes())
            );
        }
    }
    assert!(!files::is_dir(&storage, ".paperpaw/library/books"));
    assert!(!files::is_dir(&storage, ".paperpaw/library/covers"));
    assert!(!wanted(&storage));
    // Done, a new one has nothing to do.
    assert_eq!(finish(&mut Split::new(), &storage), 0);
}

#[test]
fn a_move_stopped_anywhere_goes_on_from_there() {
    let storage = MemoryStorage::new();
    let paths: Vec<String> = (0..300).map(|i| format!("books/{i}.epub")).collect();
    for (i, path) in paths.iter().enumerate() {
        storage
            .write(&old_details(path), format!("n={i}").as_bytes())
            .unwrap();
    }
    // A few steps, then PaperPaw stops (asleep, switched off): the next
    // start begins anew with what is left.
    for _ in 0..2 {
        let mut split = Split::new();
        for _ in 0..7 {
            if split.step(&storage, &[]).unwrap() {
                break;
            }
        }
    }
    let moved = paths.iter().filter(|p| storage.exists(&details(p))).count();
    assert!(moved > 0 && moved < paths.len(), "{moved}");
    finish(&mut Split::new(), &storage);
    for (i, path) in paths.iter().enumerate() {
        assert_eq!(
            storage.get(&details(path)),
            Some(format!("n={i}").into_bytes())
        );
        assert!(!storage.exists(&old_details(path)));
    }
}

#[test]
fn where_both_are_there_the_newer_stays() {
    let path = "books/a.epub";
    let storage = MemoryStorage::new()
        .with(&old_details(path), "old")
        .with(&details(path), "new");
    finish(&mut Split::new(), &storage);
    assert_eq!(storage.get(&details(path)), Some(b"new".to_vec()));
    assert!(!storage.exists(&old_details(path)));
}

#[test]
fn nothing_to_move_is_nothing_to_do() {
    let storage = MemoryStorage::new();
    assert!(!wanted(&storage));
    assert!(Split::new().step(&storage, &[]).unwrap());
    // An old folder of nothing to move goes.
    storage
        .write(".paperpaw/library/books/notes.txt", b"x")
        .unwrap();
    storage
        .write(".paperpaw/library/covers/notes.txt", b"x")
        .unwrap();
    assert!(wanted(&storage));
    finish(&mut Split::new(), &storage);
    assert!(!wanted(&storage));
}

/// A storage whose moves of names containing `refused` fail.
struct Refusing<'a> {
    inner: &'a MemoryStorage,
    refused: &'a str,
}

impl Storage for Refusing<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        match from.contains(self.refused) {
            true => Err(StorageError::Io),
            false => self.inner.rename(from, to),
        }
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
}

#[test]
fn what_cannot_be_moved_stays_where_it_is_read() {
    let inner = MemoryStorage::new();
    let paths: Vec<String> = (0..40).map(|i| format!("books/{i}.epub")).collect();
    for path in &paths {
        inner.write(&old_details(path), b"x").unwrap();
    }
    let stuck = key(&paths[7]);
    let storage = Refusing {
        inner: &inner,
        refused: &stuck,
    };
    finish(&mut Split::new(), &storage);
    assert!(inner.exists(&old_details(&paths[7])));
    for path in paths.iter().filter(|p| **p != paths[7]) {
        assert!(inner.exists(&details(path)), "{path}");
    }
    // Its folder stays, tried again at the next start.
    assert!(wanted(&inner));
    finish(&mut Split::new(), &storage);
    assert!(inner.exists(&old_details(&paths[7])));
}

#[test]
fn caches_kept_in_their_folder_itself_go_those_in_parts_stay() {
    let path = "books/a.epub";
    let k = key(path);
    let storage = MemoryStorage::new()
        .with(&old_details(path), "details")
        .with(&format!("cache/{k}/0.txt"), "old chapter")
        .with(&format!("cache/cbz-{k}"), "old pictures")
        .with(&library::kept_path("cache", "", path), "new")
        .with("cache/settings.ini", "not a book's")
        .with(&format!("notes/{k}"), "a note");
    let data = [
        BookData::new("cache", "", BookDataKind::Cache),
        BookData::new("cache", "cbz-", BookDataKind::Cache),
        // The reader's own: never cleared.
        BookData::new("notes", "", BookDataKind::UserData),
    ];
    let mut split = Split::new();
    while !split.step(&storage, &data).unwrap() {}
    assert!(!storage.exists(&format!("cache/{k}/0.txt")));
    assert!(!storage.exists(&format!("cache/cbz-{k}")));
    assert_eq!(
        storage.get(&library::kept_path("cache", "", path)),
        Some(b"new".to_vec())
    );
    assert!(storage.exists("cache/settings.ini"));
    assert!(storage.exists(&format!("notes/{k}")));
    assert_eq!(
        library::kept_path("cache/", "md-", path),
        format!("cache/{}/md-{k}", &k[..2])
    );
}

#[test]
fn only_names_of_keys_are_moved() {
    for (name, kept) in [
        ("0123456789abcdef.ini", true),
        ("0123456789abcdef.row.thumb", true),
        ("0123456789abcdef", false),
        ("0123456789ABCDEF.ini", false),
        ("0123456789abcdeg.ini", false),
        ("0123456789abcdef.", false),
        ("é123456789abcdef.ini", false),
        ("", false),
    ] {
        assert_eq!(is_kept_name(name), kept, "{name}");
    }
}
