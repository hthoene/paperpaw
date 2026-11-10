//! What plugins keep for a book follows it, a bounded step at a time: the
//! entries move to the new key without replacing, a cut at any card action is
//! finished by repeating, a removal is pieces, no memory is a status.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::net_service::fail_nth;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError, library::BookDataKind};
use std::{cell::Cell, format, string::String, vec::Vec};

/// A card that lets `allowed` mutating actions through and then none: the
/// power was cut.
struct Cut<'a> {
    card: &'a MemoryStorage,
    done: Cell<usize>,
    allowed: usize,
}

impl<'a> Cut<'a> {
    fn after(card: &'a MemoryStorage, allowed: usize) -> Self {
        Self {
            card,
            done: Cell::new(0),
            allowed,
        }
    }
    fn go(&self) -> Result<(), StorageError> {
        let n = self.done.get();
        self.done.set(n + 1);
        if n >= self.allowed {
            return Err(StorageError::Io);
        }
        Ok(())
    }
}

impl Storage for Cut<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.go()?;
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.go()?;
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.remove_dir_all(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.go()?;
        self.card.sync()
    }
}

const BOOK: &str = "Books/a.epub";
const MOVED: &str = "Books/Sub/a.epub";
const OTHER: &str = "Books/b.epub";

fn data() -> Vec<BookData> {
    std::vec![
        BookData::new(
            ".paperpaw/plugins/notes/books/vault",
            "",
            BookDataKind::UserData
        ),
        BookData::new(".paperpaw/cache", "p-", BookDataKind::Cache),
    ]
}

fn at(book: &str) -> String {
    library::key(book)
}

/// What the plugins keep for `BOOK` and for `OTHER`: a folder, a file with a
/// tail, one in the part of the key, and a cache with a prefix.
fn card() -> MemoryStorage {
    let card = MemoryStorage::new();
    let (key, other) = (at(BOOK), at(OTHER));
    let part = &key[..2];
    for (path, text) in [
        (
            format!(".paperpaw/plugins/notes/books/vault/{key}/a.txt"),
            "a",
        ),
        (
            format!(".paperpaw/plugins/notes/books/vault/{key}/deep/b.txt"),
            "b",
        ),
        (
            format!(".paperpaw/plugins/notes/books/vault/{key}.json"),
            "j",
        ),
        (
            format!(".paperpaw/plugins/notes/books/vault/{part}/{key}/c.txt"),
            "c",
        ),
        (format!(".paperpaw/cache/p-{key}/pages.bin"), "p"),
        (
            format!(".paperpaw/plugins/notes/books/vault/{other}/keep.txt"),
            "keep",
        ),
        (format!(".paperpaw/cache/p-{other}/keep.bin"), "keep"),
    ] {
        card.write(&path, text.as_bytes()).unwrap();
    }
    card
}

fn names_of(card: &MemoryStorage, book: &str) -> Vec<String> {
    let key = at(book);
    let mut found = Vec::new();
    for folder in data() {
        for split in [false, true] {
            let dir = if split {
                format!("{}/{}", folder.folder, &key[..2])
            } else {
                String::from(&*folder.folder)
            };
            let _ = card.list(&dir, &mut |entry| {
                if folder.key_of(entry.name).is_some_and(|(k, _)| k == key) {
                    found.push(format!("{dir}/{}", entry.name));
                }
            });
        }
    }
    found.sort();
    found
}

fn neighbours_intact(card: &MemoryStorage) {
    let other = at(OTHER);
    assert!(card.exists(&format!(
        ".paperpaw/plugins/notes/books/vault/{other}/keep.txt"
    )));
    assert!(card.exists(&format!(".paperpaw/cache/p-{other}/keep.bin")));
}

#[test]
fn every_entry_of_the_book_moves_to_its_new_key_and_the_old_key_keeps_nothing() {
    let card = card();
    assert_eq!(names_of(&card, BOOK).len(), 4);
    move_data(&card, &data(), BOOK, MOVED).unwrap();
    assert!(names_of(&card, BOOK).is_empty());
    let new = at(MOVED);
    let part = &new[..2];
    assert_eq!(names_of(&card, MOVED).len(), 4);
    assert!(card.exists(&format!(
        ".paperpaw/plugins/notes/books/vault/{new}/deep/b.txt"
    )));
    assert!(card.exists(&format!(".paperpaw/plugins/notes/books/vault/{new}.json")));
    assert!(card.exists(&format!(
        ".paperpaw/plugins/notes/books/vault/{part}/{new}/c.txt"
    )));
    assert!(card.exists(&format!(".paperpaw/cache/p-{new}/pages.bin")));
    neighbours_intact(&card);
    // Repeated: nothing under the old key, nothing changes.
    move_data(&card, &data(), BOOK, MOVED).unwrap();
    assert_eq!(names_of(&card, MOVED).len(), 4);
}

#[test]
fn a_move_never_replaces_what_the_new_key_already_has() {
    let card = card();
    let new = at(MOVED);
    card.write(
        &format!(".paperpaw/cache/p-{new}/own.bin"),
        b"the book's own",
    )
    .unwrap();
    move_data(&card, &data(), BOOK, MOVED).unwrap();
    // The cache of the new key is the book's own: not replaced, and the
    // old one stays where it was (the clean-up of unused books takes it).
    assert!(card.exists(&format!(".paperpaw/cache/p-{new}/own.bin")));
    assert!(!card.exists(&format!(".paperpaw/cache/p-{new}/pages.bin")));
    assert!(card.exists(&format!(".paperpaw/cache/p-{}/pages.bin", at(BOOK))));
}

#[test]
fn a_cut_after_every_card_action_is_finished_by_repeating_the_step() {
    let mut kept_old = 0;
    for allowed in 0..40 {
        let card = card();
        let cut = Cut::after(&card, allowed);
        let first = move_data(&cut, &data(), BOOK, MOVED);
        // The power came back: the same step, on the card as it is.
        move_data(&card, &data(), BOOK, MOVED).unwrap();
        assert!(names_of(&card, BOOK).is_empty(), "{allowed}");
        assert_eq!(names_of(&card, MOVED).len(), 4, "{allowed}");
        neighbours_intact(&card);
        kept_old += usize::from(first.is_err());
    }
    assert!(kept_old > 0, "some cuts did fall inside the step");
}

#[test]
fn the_data_of_a_book_is_removed_a_piece_at_a_time_and_only_its_own() {
    let card = card();
    let key = at(BOOK);
    for n in 0..150 {
        card.write(&format!(".paperpaw/cache/p-{key}/bulk/f{n}.bin"), b"x")
            .unwrap();
    }
    let mut pieces = 0;
    while remove_data_piece(&card, &data(), BOOK).unwrap() == Progress::More {
        pieces += 1;
        assert!(pieces < 100, "never ends");
    }
    assert!(
        pieces >= 4,
        "folders and a file: a piece each, at least: {pieces}"
    );
    assert!(names_of(&card, BOOK).is_empty());
    neighbours_intact(&card);
    // Nothing is left: Done at once.
    assert_eq!(
        remove_data(&card, &data(), BOOK, 3).unwrap(),
        Progress::Done
    );
    // A bounded call that cannot finish says so.
    let card = self::card();
    assert_eq!(
        remove_data(&card, &data(), BOOK, 1).unwrap(),
        Progress::More
    );
}

#[test]
fn a_cut_during_a_removal_is_finished_by_repeating_it() {
    for allowed in 0..30 {
        let card = card();
        let cut = Cut::after(&card, allowed);
        let _ = remove_data(&cut, &data(), BOOK, REMOVE_PIECES);
        assert_eq!(
            remove_data(&card, &data(), BOOK, REMOVE_PIECES).unwrap(),
            Progress::Done,
            "{allowed}"
        );
        assert!(names_of(&card, BOOK).is_empty(), "{allowed}");
        neighbours_intact(&card);
    }
}

#[test]
fn no_memory_is_a_status_and_the_repeat_goes_on() {
    for nth in 1..40 {
        let card = card();
        fail_nth(nth);
        let first = move_data(&card, &data(), BOOK, MOVED);
        fail_nth(0);
        if first.is_err() {
            assert_eq!(first, Err(ERR_NO_MEMORY), "{nth}");
        }
        move_data(&card, &data(), BOOK, MOVED).unwrap();
        assert!(names_of(&card, BOOK).is_empty(), "{nth}");
        assert_eq!(names_of(&card, MOVED).len(), 4, "{nth}");
        neighbours_intact(&card);
    }
    for nth in 1..40 {
        let card = card();
        fail_nth(nth);
        let first = remove_data(&card, &data(), BOOK, REMOVE_PIECES);
        fail_nth(0);
        if first.is_err() {
            assert_eq!(first, Err(ERR_NO_MEMORY), "{nth}");
        }
        assert_eq!(
            remove_data(&card, &data(), BOOK, REMOVE_PIECES).unwrap(),
            Progress::Done,
            "{nth}"
        );
        assert!(names_of(&card, BOOK).is_empty(), "{nth}");
        neighbours_intact(&card);
    }
}

#[test]
fn a_folder_that_is_not_there_keeps_nothing_and_is_no_error() {
    let card = MemoryStorage::new();
    move_data(&card, &data(), BOOK, MOVED).unwrap();
    assert_eq!(
        remove_data(&card, &data(), BOOK, 2).unwrap(),
        Progress::Done
    );
}
