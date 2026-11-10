//! What plugins keep for a book follows it, a bounded step at a time: the
//! entries move to the new key without replacing, a cut at any card action is
//! finished by repeating, a removal is pieces, no memory is a status.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::net_service::fail_nth;
use paperpaw_sdk::{
    DirEntry, MemoryStorage, Storage, StorageError,
    library::{self, BookDataKind},
};
use std::{cell::Cell, format, string::String, vec::Vec};

/// A card that refuses to rename or remove an entry whose path holds `name`,
/// with `error`: for good (`InvalidPath`) or a transient one (`Io`).
struct Refuse<'a> {
    card: &'a MemoryStorage,
    name: &'static str,
    error: StorageError,
}

impl Storage for Refuse<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        if p.contains(self.name) {
            return Err(self.error);
        }
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        if f.contains(self.name) {
            return Err(self.error);
        }
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.card.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove_dir_all(p)
    }
}

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
    while remove_data(&card, &data(), BOOK, 1).unwrap() == Progress::More {
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

/// `count` per-page files of `BOOK` in one folder (`<key>.p<n>.png`).
fn pages(card: &MemoryStorage, count: usize) {
    let key = at(BOOK);
    for n in 0..count {
        card.write(
            &format!(".paperpaw/plugins/notes/books/vault/{key}.p{n}.png"),
            b"x",
        )
        .unwrap();
    }
}

#[test]
fn a_book_with_more_entries_than_a_batch_moves_and_is_removed_whole() {
    for count in [63, 64, 65, 200] {
        let card = card();
        pages(&card, count);
        move_data(&card, &data(), BOOK, MOVED).unwrap();
        assert!(names_of(&card, BOOK).is_empty(), "{count}");
        assert_eq!(names_of(&card, MOVED).len(), count + 4, "{count}");
        neighbours_intact(&card);
        assert_eq!(
            remove_data(&card, &data(), MOVED, 10_000).unwrap(),
            Progress::Done,
            "{count}"
        );
        assert!(names_of(&card, MOVED).is_empty(), "{count}");
        neighbours_intact(&card);
    }
}

#[test]
fn an_entry_the_card_refuses_for_good_stays_and_the_rest_moves() {
    // One page is refused wherever it is in the listing (first batch, a
    // batch boundary, a later batch).
    for (count, name) in [(65, ".p0.png"), (200, ".p64.png"), (200, ".p150.png")] {
        let card = card();
        pages(&card, count);
        let refusing = Refuse {
            card: &card,
            name,
            error: StorageError::InvalidPath,
        };
        move_data(&refusing, &data(), BOOK, MOVED).unwrap();
        assert_eq!(names_of(&card, BOOK).len(), 1, "{count} {name}");
        assert_eq!(names_of(&card, MOVED).len(), count + 3, "{count} {name}");
        neighbours_intact(&card);
    }
}

#[test]
fn a_card_that_refuses_every_rename_ends_the_step_with_everything_left() {
    let card = card();
    pages(&card, 200);
    let refusing = Refuse {
        card: &card,
        name: "",
        error: StorageError::InvalidPath,
    };
    move_data(&refusing, &data(), BOOK, MOVED).unwrap();
    assert_eq!(names_of(&card, BOOK).len(), 204);
    assert!(names_of(&card, MOVED).is_empty());
}

#[test]
fn a_transient_card_error_is_the_status_and_the_repeat_finishes() {
    let card = card();
    pages(&card, 100);
    let failing = Refuse {
        card: &card,
        name: ".p77.png",
        error: StorageError::Io,
    };
    assert_eq!(
        move_data(&failing, &data(), BOOK, MOVED),
        Err(status(StorageError::Io))
    );
    move_data(&card, &data(), BOOK, MOVED).unwrap();
    assert!(names_of(&card, BOOK).is_empty());
    assert_eq!(names_of(&card, MOVED).len(), 104);
}

#[test]
fn a_removal_leaves_what_the_card_refuses_and_is_done() {
    let card = card();
    pages(&card, 200);
    let refusing = Refuse {
        card: &card,
        name: ".p100.png",
        error: StorageError::InvalidPath,
    };
    assert_eq!(
        remove_data(&refusing, &data(), BOOK, 10_000).unwrap(),
        Progress::Done
    );
    assert_eq!(names_of(&card, BOOK).len(), 1);
    neighbours_intact(&card);
    // A transient error is the status.
    let failing = Refuse {
        card: &card,
        name: ".p100.png",
        error: StorageError::Io,
    };
    assert_eq!(
        remove_data(&failing, &data(), BOOK, 10_000),
        Err(status(StorageError::Io))
    );
}

/// Counts the folders listed; everything else goes to the card.
struct Listings<'a> {
    card: &'a MemoryStorage,
    lists: Cell<usize>,
}

impl Storage for Listings<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.lists.set(self.lists.get() + 1);
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.card.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove_dir_all(p)
    }
}

/// A library of 1,000 books moved one after the other (a folder move): each
/// book lists a folder once per place its data can be (the folder and the part
/// of the key), not once more for a batch that was not full.
#[test]
fn a_move_lists_each_folder_once_for_a_thousand_books() {
    let card = MemoryStorage::new();
    let books: Vec<String> = (0..1000).map(|n| format!("Books/b{n}.epub")).collect();
    for book in &books {
        let key = at(book);
        card.write(
            &format!(".paperpaw/plugins/notes/books/vault/{key}/a.txt"),
            b"a",
        )
        .unwrap();
        card.write(&format!(".paperpaw/cache/p-{key}/pages.bin"), b"p")
            .unwrap();
    }
    let counted = Listings {
        card: &card,
        lists: Cell::new(0),
    };
    for book in &books {
        let moved = format!("Books/Sub/{}", &book[6..]);
        move_data(&counted, &data(), book, &moved).unwrap();
        assert!(names_of(&card, book).is_empty());
    }
    // Two folders, each listed in its own place and in the key's part (4),
    // and the new name of each entry looked at as a folder (2): not once more
    // for a batch that was not full.
    assert_eq!(counted.lists.get(), 1000 * 6);
}

/// The `core` plugin's event handlers (interface 6): the same steps, by what
/// one handler call may do.
#[test]
fn the_event_handlers_move_a_large_book_whole_and_remove_a_bounded_part() {
    for count in [65, 200] {
        let card = card();
        pages(&card, count);
        follow_move(&card, &data(), BOOK, MOVED);
        assert!(names_of(&card, BOOK).is_empty(), "{count}");
        assert_eq!(names_of(&card, MOVED).len(), count + 4, "{count}");
        follow_removal(&card, &data(), MOVED);
        // At most REMOVE_PIECES pieces; the rest is for the clean-up.
        let left = names_of(&card, MOVED).len();
        assert!(left < count + 4, "{count}");
        assert!(left + REMOVE_PIECES as usize >= count + 4, "{count}");
        neighbours_intact(&card);
    }
}

#[test]
fn the_event_handlers_end_with_everything_left_when_the_card_refuses_for_good() {
    let card = card();
    pages(&card, 200);
    let refusing = Refuse {
        card: &card,
        name: "",
        error: StorageError::InvalidPath,
    };
    follow_move(&refusing, &data(), BOOK, MOVED);
    follow_removal(&refusing, &data(), BOOK);
    assert_eq!(names_of(&card, BOOK).len(), 204);
}

/// A document's cache as the host makes it: pages, and the stamp that makes it
/// count as whole.
fn doc_cache(card: &MemoryStorage, book: &str, pages: usize) {
    let key = at(book);
    for n in 0..pages {
        card.write(
            &format!(".paperpaw/cache/{key}/epub/c1/pages/p{n}.bin"),
            b"x",
        )
        .unwrap();
    }
    card.write(
        &format!(".paperpaw/cache/{key}/epub/c1/.host/.complete"),
        b"k",
    )
    .unwrap();
}

fn stamped(card: &MemoryStorage, book: &str) -> bool {
    card.exists(&format!(
        ".paperpaw/cache/{}/epub/c1/.host/.complete",
        at(book)
    ))
}

fn pages_left(card: &MemoryStorage, book: &str) -> usize {
    let mut n = 0;
    let _ = card.list(
        &format!(".paperpaw/cache/{}/epub/c1/pages", at(book)),
        &mut |_| n += 1,
    );
    n
}

/// The host's own cache of a document: `.paperpaw/cache/<key>/`, not renamed
/// (its folder is named by the key) but removed, and only that book's.
#[test]
fn the_document_cache_of_a_book_is_removed_and_only_its_own() {
    let card = card();
    doc_cache(&card, BOOK, 150);
    doc_cache(&card, OTHER, 3);
    let mut calls = 0;
    loop {
        let mut budget = 1;
        if remove_doc_cache(&card, BOOK, &mut budget).unwrap() == Progress::Done {
            break;
        }
        calls += 1;
        assert!(calls < 100, "never ends");
    }
    assert!(calls >= 2, "bounded pieces: {calls}");
    assert_eq!(pages_left(&card, BOOK), 0);
    assert!(stamped(&card, OTHER) && pages_left(&card, OTHER) == 3);
    // Not renamed to the new key by a move; gone with the old one.
    let card = self::card();
    doc_cache(&card, BOOK, 3);
    follow_move(&card, &data(), BOOK, MOVED);
    assert!(!stamped(&card, BOOK) && pages_left(&card, BOOK) == 0);
    assert!(!stamped(&card, MOVED));
}

/// Stamps first: at every cut, and when the budget ends a removal in the
/// middle, a cache is whole or has no stamp, so an open never takes pages
/// that are gone for a finished cache.
#[test]
fn a_cache_that_is_cut_down_in_the_middle_has_no_stamp() {
    for allowed in 0..40 {
        let card = card();
        doc_cache(&card, BOOK, 150);
        let cut = Cut::after(&card, allowed);
        let mut budget = 100;
        let _ = remove_doc_cache(&cut, BOOK, &mut budget);
        assert!(
            !stamped(&card, BOOK) || pages_left(&card, BOOK) == 150,
            "{allowed}: a stamp over missing pages"
        );
    }
    for pieces in 0..4 {
        let card = card();
        doc_cache(&card, BOOK, 150);
        let mut budget = pieces;
        let _ = remove_doc_cache(&card, BOOK, &mut budget);
        assert!(
            !stamped(&card, BOOK) || pages_left(&card, BOOK) == 150,
            "{pieces} pieces: a stamp over missing pages"
        );
    }
    // The event handler gives up after its pieces: no stamp stays.
    let card = card();
    doc_cache(&card, BOOK, 1000);
    follow_removal(&card, &data(), BOOK);
    assert!(!stamped(&card, BOOK));
}
