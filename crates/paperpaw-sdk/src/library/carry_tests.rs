//! A card error in any step of [`try_moved`] or [`try_removed`] is
//! returned, and the same call run again finishes as one run without it;
//! an error no retry changes lets the other parts go on.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use crate::MemoryStorage;
use crate::document::ImageRef;
use crate::library::{book, favourites, opened_books, save, set_favourite, set_field};
use alloc::format;
use core::cell::{Cell, RefCell};
use paperpaw_platform::DirEntry;

/// A memory card whose chosen renames, removes or writes (counted from 0)
/// fail, each with its error.
struct Faulty {
    inner: MemoryStorage,
    faults: RefCell<Vec<(usize, StorageError)>>,
    calls: Cell<usize>,
    writes: Cell<usize>,
    /// Every read fails with this error while set.
    fail_reads: Cell<Option<StorageError>>,
}

impl Faulty {
    fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            faults: RefCell::new(Vec::new()),
            calls: Cell::new(0),
            writes: Cell::new(0),
            fail_reads: Cell::new(None),
        }
    }
    /// The `at`th changing call fails with `error`.
    fn fail(&self, at: usize, error: StorageError) {
        self.faults.borrow_mut().push((at, error));
    }
    /// No call fails any more.
    fn heal(&self) {
        self.faults.borrow_mut().clear();
    }
    /// Counts a changing call; a chosen one fails.
    fn step(&self) -> Result<(), StorageError> {
        let n = self.calls.get();
        self.calls.set(n + 1);
        match self.faults.borrow().iter().find(|(at, _)| *at == n) {
            Some((_, error)) => Err(*error),
            None => Ok(()),
        }
    }
}

impl Storage for Faulty {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if let Some(error) = self.fail_reads.get() {
            return Err(error);
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

/// A card with two favourite books, one opened, `a.epub` with details and
/// a cover.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new()
        .with("a.epub", "1")
        .with("b.epub", "2")
        .with(&cover_file("a.epub"), "cover");
    set_favourite(&storage, "a.epub", true);
    set_favourite(&storage, "b.epub", true);
    set_field(&storage, "a.epub", "x.y", "1");
    storage.write(&opened_file(), b"a.epub\nb.epub").unwrap();
    storage
}

/// What the library knows after the move of `a.epub` to `c.epub`: the
/// lists, the details' field and whether the cover is at `c.epub`'s place.
type MovedState = (Vec<String>, Vec<String>, Option<String>, bool);

fn moved_state(storage: &dyn Storage) -> MovedState {
    (
        favourites(storage),
        opened_books(storage),
        book(storage, "c.epub").and_then(|b| b.field("x.y").map(ToString::to_string)),
        storage.exists(&cover_file("c.epub")) && !storage.exists(&cover_file("a.epub")),
    )
}

#[test]
fn a_move_carries_the_details_and_the_lists_and_a_repeat_changes_nothing() {
    let storage = Faulty::new(card());
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    storage.inner.rename("a.epub", "c.epub").unwrap();
    let (fav, opened, field, cover) = moved_state(&storage);
    assert_eq!(fav, ["b.epub", "c.epub"]);
    assert_eq!(opened, ["c.epub", "b.epub"]);
    assert_eq!(field.as_deref(), Some("1"));
    assert!(cover);
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
        storage.fail(at, StorageError::Io);
        assert_eq!(
            try_moved(&storage, "a.epub", "c.epub"),
            Err(StorageError::Io),
            "step {at}"
        );
        storage.heal();
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
    assert!(!clean.exists(&cover_file("a.epub")));
    for at in 0..steps {
        let storage = Faulty::new(card());
        storage.fail(at, StorageError::Io);
        assert_eq!(
            try_removed(&storage, "a.epub"),
            Err(StorageError::Io),
            "step {at}"
        );
        storage.heal();
        try_removed(&storage, "a.epub").unwrap();
        assert_eq!(favourites(&storage), ["b.epub"], "step {at}");
        assert_eq!(opened_books(&storage), ["b.epub"], "step {at}");
        assert!(book(&storage, "a.epub").is_none(), "step {at}");
        assert!(!storage.exists(&cover_file("a.epub")), "step {at}");
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
fn a_list_from_a_newer_version_is_left_as_it_is_and_fails_a_removal() {
    let newer = "# format 9\na.epub\n";
    let storage = Faulty::new(card().with(&favourites_file(), newer));
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    assert_eq!(
        try_removed(&storage, "c.epub"),
        Err(StorageError::InvalidData)
    );
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
    // As a host calls it: the book itself is at `to` already.
    let clean = Faulty::new(card());
    clean.inner.rename("a.epub", "c.epub").unwrap();
    try_moved(&clean, "a.epub", "c.epub").unwrap();
    let want = moved_state(&clean);
    let storage = Faulty::new(card());
    storage.inner.rename("a.epub", "c.epub").unwrap();
    storage.fail_reads.set(Some(StorageError::Io));
    assert_eq!(
        try_moved(&storage, "a.epub", "c.epub"),
        Err(StorageError::Io)
    );
    assert_eq!(try_removed(&storage, "b.epub"), Err(StorageError::Io));
    storage.fail_reads.set(None);
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

#[test]
fn a_permanent_error_in_any_step_of_a_move_is_returned_after_the_other_parts() {
    let clean = Faulty::new(card());
    try_moved(&clean, "a.epub", "c.epub").unwrap();
    let steps = clean.calls.get();
    let want = moved_state(&clean);
    for at in 0..steps {
        let storage = Faulty::new(card());
        storage.fail(at, StorageError::InvalidPath);
        assert_eq!(
            try_moved(&storage, "a.epub", "c.epub"),
            Err(StorageError::InvalidPath),
            "step {at}"
        );
        storage.heal();
        try_moved(&storage, "a.epub", "c.epub").unwrap();
        assert_eq!(moved_state(&storage), want, "step {at}");
    }
}

#[test]
fn details_that_cannot_move_leave_the_cover_and_the_lists_carried() {
    let storage = Faulty::new(card());
    // Step 0 is the details' rename.
    storage.fail(0, StorageError::InvalidPath);
    assert_eq!(
        try_moved(&storage, "a.epub", "c.epub"),
        Err(StorageError::InvalidPath)
    );
    let (fav, opened, field, cover) = moved_state(&storage);
    assert_eq!(fav, ["b.epub", "c.epub"]);
    assert_eq!(opened, ["c.epub", "b.epub"]);
    assert!(cover);
    // The details stay where they were, the only copy.
    assert_eq!(field, None);
    assert!(storage.exists(&book_file("a.epub")));
}

#[test]
fn an_error_a_retry_may_cure_wins_over_a_permanent_one() {
    let storage = Faulty::new(card());
    storage.fail(0, StorageError::Unsupported);
    for at in 1..64 {
        storage.fail(at, StorageError::Full);
    }
    assert_eq!(
        try_moved(&storage, "a.epub", "c.epub"),
        Err(StorageError::Full)
    );
    let before = favourites(&card());
    for error in [
        StorageError::Io,
        StorageError::OutOfMemory,
        StorageError::Other(7),
    ] {
        let storage = Faulty::new(card());
        storage.fail(0, error);
        assert_eq!(try_moved(&storage, "a.epub", "c.epub"), Err(error));
        // It stopped there: nothing else was changed.
        assert_eq!(favourites(&storage), before);
        assert_eq!(storage.calls.get(), 1);
    }
}

#[test]
fn a_removal_goes_on_past_a_permanent_error() {
    let storage = Faulty::new(card());
    // Step 0 is the details' removal.
    storage.fail(0, StorageError::TooLarge);
    assert_eq!(try_removed(&storage, "a.epub"), Err(StorageError::TooLarge));
    assert_eq!(favourites(&storage), ["b.epub"]);
    assert_eq!(opened_books(&storage), ["b.epub"]);
    assert!(!storage.exists(&cover_file("a.epub")));
    assert!(storage.exists(&book_file("a.epub")));
    let clean = Faulty::new(card());
    try_removed(&clean, "a.epub").unwrap();
    for at in 0..clean.calls.get() {
        let storage = Faulty::new(card());
        storage.fail(at, StorageError::InvalidData);
        assert_eq!(
            try_removed(&storage, "a.epub"),
            Err(StorageError::InvalidData),
            "step {at}"
        );
        storage.heal();
        try_removed(&storage, "a.epub").unwrap();
        assert_eq!(favourites(&storage), ["b.epub"], "step {at}");
        assert!(book(&storage, "a.epub").is_none(), "step {at}");
    }
}

#[test]
fn an_older_copy_is_not_moved_over_details_that_cannot_move() {
    let storage = Faulty::new(card().with(&old_book_file("a.epub"), "# old"));
    // Step 0 is the details' rename from their current place.
    storage.fail(0, StorageError::InvalidPath);
    assert_eq!(
        try_moved(&storage, "a.epub", "c.epub"),
        Err(StorageError::InvalidPath)
    );
    assert!(storage.exists(&book_file("a.epub")));
    assert!(storage.exists(&old_book_file("a.epub")));
    assert!(!storage.exists(&book_file("c.epub")));
}

#[test]
fn a_permanent_read_error_lets_the_renames_go_on() {
    let storage = Faulty::new(card());
    storage.inner.rename("a.epub", "c.epub").unwrap();
    storage.fail_reads.set(Some(StorageError::InvalidPath));
    assert_eq!(
        try_moved(&storage, "a.epub", "c.epub"),
        Err(StorageError::InvalidPath)
    );
    // The details' read and both lists' reads failed; the files moved.
    assert!(storage.exists(&book_file("c.epub")));
    assert!(storage.exists(&cover_file("c.epub")));
    assert!(!storage.exists(&book_file("a.epub")));
    storage.fail_reads.set(None);
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    assert_eq!(favourites(&storage), ["b.epub", "c.epub"]);
}

#[test]
fn damaged_data_on_a_read_is_left_out_without_an_error() {
    // The data layer reads InvalidData as unreadable: left as it is, as a
    // list of a newer version, and not an error of the carry.
    let storage = Faulty::new(card());
    storage.inner.rename("a.epub", "c.epub").unwrap();
    storage.fail_reads.set(Some(StorageError::InvalidData));
    try_moved(&storage, "a.epub", "c.epub").unwrap();
    assert!(storage.exists(&book_file("c.epub")));
    assert!(storage.exists(&cover_file("c.epub")));
}

#[test]
fn an_unreadable_list_fails_a_removal_after_the_other_parts() {
    for list in [opened_file(), favourites_file()] {
        let storage = Faulty::new(card().with(&list, "# format 9\na.epub\n"));
        assert_eq!(
            try_removed(&storage, "a.epub"),
            Err(StorageError::InvalidData)
        );
        // The rest is forgotten; the list is as it was; a repeat gives the
        // same answer.
        assert!(!storage.exists(&book_file("a.epub")));
        assert!(!storage.exists(&cover_file("a.epub")));
        assert_eq!(
            storage.inner.get(&list).unwrap(),
            b"# format 9\na.epub\n".to_vec()
        );
        assert_eq!(
            try_removed(&storage, "a.epub"),
            Err(StorageError::InvalidData)
        );
    }
}
