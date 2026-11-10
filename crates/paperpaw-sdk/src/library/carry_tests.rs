//! A card error in any step of [`try_moved`] or [`try_removed`] is
//! returned, and the same call run again finishes as one run without it.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use crate::MemoryStorage;
use crate::document::ImageRef;
use crate::library::{book, favourites, opened_books, save, set_favourite, set_field};
use alloc::format;
use core::cell::Cell;
use paperpaw_platform::DirEntry;

/// A memory card whose `n`th rename, remove or write (from 0) fails.
struct Faulty {
    inner: MemoryStorage,
    fail_at: Cell<Option<usize>>,
    calls: Cell<usize>,
    writes: Cell<usize>,
    /// Every read fails while set.
    fail_reads: Cell<bool>,
}

impl Faulty {
    fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            fail_at: Cell::new(None),
            calls: Cell::new(0),
            writes: Cell::new(0),
            fail_reads: Cell::new(false),
        }
    }
    /// Counts a changing call; the chosen one fails.
    fn step(&self) -> Result<(), StorageError> {
        let n = self.calls.get();
        self.calls.set(n + 1);
        match self.fail_at.get() {
            Some(at) if at == n => Err(StorageError::Io),
            _ => Ok(()),
        }
    }
}

impl Storage for Faulty {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.fail_reads.get() {
            return Err(StorageError::Io);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.step()?;
        self.writes.set(self.writes.get() + 1);
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.step()?;
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.step()?;
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.step()?;
        self.inner.rename(from, to)
    }
}

/// A card with two favourite books, one opened, `a.epub` with details.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new().with("a.epub", "1").with("b.epub", "2");
    set_favourite(&storage, "a.epub", true);
    set_favourite(&storage, "b.epub", true);
    set_field(&storage, "a.epub", "x.y", "1");
    storage.write(&opened_file(), b"a.epub\nb.epub").unwrap();
    storage
}

/// What the library knows after the move of `a.epub` to `c.epub`.
fn moved_state(storage: &dyn Storage) -> (Vec<String>, Vec<String>, Option<String>) {
    (
        favourites(storage),
        opened_books(storage),
        book(storage, "c.epub").and_then(|b| b.field("x.y").map(ToString::to_string)),
    )
}

#[test]
fn a_move_carries_the_details_and_the_lists_and_a_repeat_changes_nothing() {
    let storage = Faulty::new(card());
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    storage.inner.rename("a.epub", "c.epub").unwrap();
    let (fav, opened, field) = moved_state(&storage);
    assert_eq!(fav, ["b.epub", "c.epub"]);
    assert_eq!(opened, ["c.epub", "b.epub"]);
    assert_eq!(field.as_deref(), Some("1"));
    let writes = storage.writes.get();
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    // Renames of what is gone and removes of what is not there only: no
    // list or details written again.
    assert_eq!(storage.writes.get(), writes);
    assert_eq!(moved_state(&storage).0, ["b.epub", "c.epub"]);
}

#[test]
fn every_failing_step_of_a_move_is_returned_and_a_repeat_finishes_it() {
    let clean = Faulty::new(card());
    try_moved(&clean, "a.epub", "c.epub").unwrap();
    let steps = clean.calls.get();
    assert!(steps >= 4, "{steps}");
    let want = moved_state(&clean);
    for at in 0..steps {
        let storage = Faulty::new(card());
        storage.fail_at.set(Some(at));
        assert_eq!(
            try_moved(&storage, "a.epub", "c.epub"),
            Err(StorageError::Io),
            "step {at}"
        );
        storage.fail_at.set(None);
        try_moved(&storage, "a.epub", "c.epub").unwrap();
        assert_eq!(moved_state(&storage), want, "step {at}");
    }
}

#[test]
fn every_failing_step_of_a_removal_is_returned_and_a_repeat_finishes_it() {
    let clean = Faulty::new(card());
    try_removed(&clean, "a.epub").unwrap();
    let steps = clean.calls.get();
    assert_eq!(favourites(&clean), ["b.epub"]);
    assert_eq!(opened_books(&clean), ["b.epub"]);
    assert!(book(&clean, "a.epub").is_none());
    for at in 0..steps {
        let storage = Faulty::new(card());
        storage.fail_at.set(Some(at));
        assert_eq!(
            try_removed(&storage, "a.epub"),
            Err(StorageError::Io),
            "step {at}"
        );
        storage.fail_at.set(None);
        try_removed(&storage, "a.epub").unwrap();
        assert_eq!(favourites(&storage), ["b.epub"], "step {at}");
        assert_eq!(opened_books(&storage), ["b.epub"], "step {at}");
        assert!(book(&storage, "a.epub").is_none(), "step {at}");
    }
}

#[test]
fn a_book_the_library_does_not_know_is_moved_and_removed_without_a_write() {
    let storage = Faulty::new(card());
    let before = favourites(&storage);
    try_moved(&storage, "none.epub", "other.epub").unwrap();
    try_removed(&storage, "none.epub").unwrap();
    assert_eq!(favourites(&storage), before);
    let empty = Faulty::new(MemoryStorage::new());
    try_moved(&empty, "a.epub", "c.epub").unwrap();
    try_removed(&empty, "a.epub").unwrap();
    assert!(empty.inner.get(&favourites_file()).is_none());
}

#[test]
fn a_list_from_a_newer_version_is_left_as_it_is() {
    let newer = "# format 9\na.epub\n";
    let storage = Faulty::new(card().with(&favourites_file(), newer));
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    try_removed(&storage, "c.epub").unwrap();
    assert_eq!(
        storage.inner.get(&favourites_file()).unwrap(),
        newer.as_bytes()
    );
}

#[test]
fn a_cover_kept_by_the_books_data_is_named_where_the_book_is_now() {
    let storage = Faulty::new(card());
    let mut details = book(&storage, "a.epub").unwrap();
    details.cover = Some(ImageRef {
        path: format!("format/books/{}/cover.gray", key("a.epub")),
        width: 3,
        height: 4,
    });
    save(&storage, "a.epub", &details).unwrap();
    storage.inner.rename("a.epub", "c.epub").unwrap();
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    let cover = book(&storage, "c.epub").unwrap().cover.unwrap();
    assert_eq!(
        cover.path,
        format!("format/books/{}/cover.gray", key("c.epub"))
    );
}

#[test]
fn a_failing_read_is_returned_and_a_repeat_finishes_the_move() {
    let clean = Faulty::new(card());
    try_moved(&clean, "a.epub", "c.epub").unwrap();
    let want = moved_state(&clean);
    let storage = Faulty::new(card());
    storage.fail_reads.set(true);
    assert_eq!(
        try_moved(&storage, "a.epub", "c.epub"),
        Err(StorageError::Io)
    );
    assert_eq!(try_removed(&storage, "b.epub"), Err(StorageError::Io));
    storage.fail_reads.set(false);
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    assert_eq!(moved_state(&storage), want);
}

#[test]
fn details_of_a_newer_version_are_left_as_they_are() {
    let newer = b"# format 9\n[book]\nsize = 1\n".to_vec();
    let storage = Faulty::new(card().with(&book_file("c.epub"), newer.clone()));
    storage.inner.remove(&book_file("a.epub")).unwrap();
    storage.inner.rename("a.epub", "c.epub").unwrap();
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    assert_eq!(storage.inner.get(&book_file("c.epub")).unwrap(), newer);
}
