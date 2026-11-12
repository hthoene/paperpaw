//! Making the library's catalog as the kernel does: a walk takes the
//! books whose files did not change from the old catalog, the log noted
//! before goes with a new catalog, a book removed and sent again starts
//! afresh, catching up takes books that came without walking, and an
//! error leaves the old catalog as it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::borrow::Cow;
use std::cell::Cell;

use paperpaw_sdk::builtin::events::Reading;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::library::catalog::{self, Rewrite, Status, Walk};
use paperpaw_sdk::library::{self, index, index::Index};
use paperpaw_sdk::{DirEntry, Env, FormatSpec, Formats, Id, MemoryStorage, Storage, StorageError};

fn harness(storage: MemoryStorage) -> common::Harness {
    let mut h = common::Harness::new();
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.txt"),
            "TXT",
            vec![Cow::Borrowed("txt")],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    h.formats = formats;
    h.storage = storage;
    h
}

fn walk(env: &Env<'_>) {
    let mut walk = Walk::start(env, Vec::new()).unwrap();
    while !walk.step(env).unwrap() {}
}

fn catch_up(env: &Env<'_>, fresh: &[&str]) {
    let fresh = fresh.iter().map(|p| p.to_string()).collect();
    let mut rewrite = Rewrite::start(env, fresh).unwrap();
    while !rewrite.step(env).unwrap() {}
}

/// The books of the catalog: path and how far read, by path.
fn books(storage: &dyn Storage) -> Vec<(String, Option<u8>)> {
    let mut books = Vec::new();
    catalog::each(storage, &mut |r| books.push((r.path, r.status.progress)));
    books.sort();
    books
}

fn details(path: &str) -> String {
    let key = library::key(path);
    format!(".paperpaw/library/details/{}/{key}.ini", &key[..2])
}

fn reading(path: &str, progress: f32) -> Reading {
    Reading::new(path.into(), String::new(), progress, 0, 0, String::new())
}

/// Bytes of a source that never draws the same twice.
fn drawn(buf: &mut [u8]) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0x5eed);
    let next = NEXT.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    for (byte, b) in buf.iter_mut().zip(next.to_le_bytes().iter().cycle()) {
        *byte = *b;
    }
    true
}

#[test]
fn a_catalog_made_where_one_went_is_not_taken_for_it() {
    let mut h = harness(MemoryStorage::new().with("books/a.txt", "one"));
    h.random = paperpaw_sdk::random::Random::new(drawn);
    let env = h.env();
    walk(&env);
    let generation = catalog::generation(&h.storage).unwrap();
    let version = index::catalog_version(&h.storage).unwrap();
    // The catalog gone (the books as they were): made anew of the same
    // length, but not of the same generation, so not of the same version
    // to what kept the old one's (a sorted index of it).
    h.storage.remove(".paperpaw/library/catalog.txt").unwrap();
    walk(&env);
    assert_ne!(catalog::generation(&h.storage), Some(generation));
    assert_ne!(index::catalog_version(&h.storage), Some(version));
    // Made again from one there is: the generation after.
    let made = catalog::generation(&h.storage).unwrap();
    h.storage.write("books/a.txt", b"one more").unwrap();
    walk(&env);
    assert_eq!(catalog::generation(&h.storage), Some(made + 1));
}

#[test]
fn a_walk_takes_books_whose_files_did_not_change_from_the_old_catalog() {
    let h = harness(
        MemoryStorage::new()
            .with("books/a.txt", "one")
            .with("books/Sub/b.txt", "two")
            .with("books/.hidden.txt", "no")
            .with("books/c.pdf", "not a book here"),
    );
    let env = h.env();
    walk(&env);
    let found = books(&h.storage);
    assert_eq!(
        found,
        [
            ("books/Sub/b.txt".to_string(), None),
            ("books/a.txt".to_string(), None)
        ]
    );
    assert!(h.storage.exists(&details("books/a.txt")), "indexed");
    let first = catalog::generation(&h.storage).unwrap();
    // The details gone, the files as they were: taken from the catalog,
    // nothing read or opened.
    h.storage.remove(&details("books/a.txt")).unwrap();
    walk(&env);
    assert_eq!(books(&h.storage), found);
    assert!(!h.storage.exists(&details("books/a.txt")));
    // Nothing changed: the catalog stays, of the same generation.
    assert_eq!(catalog::generation(&h.storage), Some(first));
    // A file of another size is read anew; the listing kept goes.
    h.storage.write("books/a.txt", b"one more").unwrap();
    walk(&env);
    assert!(h.storage.exists(&details("books/a.txt")));
    assert_eq!(catalog::generation(&h.storage), Some(first + 1));
    assert!(!h.storage.exists(".paperpaw/library/walk.list"));
    assert!(!h.storage.exists(".paperpaw/library/catalog.tmp"));
}

#[test]
fn what_was_noted_before_a_walk_goes_with_it() {
    let h = harness(
        MemoryStorage::new()
            .with("books/a.txt", "one")
            .with("books/b.txt", "two"),
    );
    let env = h.env();
    walk(&env);
    // Opened at 10 % (noted), read on to 45 % without closing (the
    // details only), b finished (noted).
    library::record(&env, &reading("books/a.txt", 10.0), true);
    library::record(&env, &reading("books/a.txt", 45.0), false);
    library::set_finished(&h.storage, "books/b.txt", true);
    assert_eq!(books(&h.storage)[0].1, Some(10));
    walk(&env);
    // The details' 45 %, not the 10 % noted before; b as noted; nothing
    // left in the log to override them.
    assert_eq!(
        books(&h.storage),
        [
            ("books/a.txt".to_string(), Some(45)),
            ("books/b.txt".to_string(), Some(100))
        ]
    );
    assert!(catalog::changes(&h.storage).is_empty());
}

#[test]
fn a_book_removed_and_sent_again_starts_afresh() {
    let h = harness(
        MemoryStorage::new()
            .with("books/a.txt", "one")
            .with("books/b.txt", "two"),
    );
    let env = h.env();
    walk(&env);
    library::record(&env, &reading("books/b.txt", 80.0), true);
    assert_eq!(books(&h.storage)[1].1, Some(80));
    // Removed, as the kernel does it: left out at once.
    h.storage.remove("books/b.txt").unwrap();
    library::removed(&h.storage, "books/b.txt");
    catalog::forget(&h.storage, "books/b.txt").unwrap();
    assert_eq!(books(&h.storage).len(), 1);
    // Sent again: caught up, without its old progress.
    h.storage.write("books/b.txt", b"two").unwrap();
    let before = catalog::generation(&h.storage).unwrap();
    catch_up(&env, &["books/b.txt"]);
    assert_eq!(
        books(&h.storage),
        [
            ("books/a.txt".to_string(), None),
            ("books/b.txt".to_string(), None)
        ]
    );
    assert_eq!(catalog::generation(&h.storage), Some(before + 1));
    assert!(catalog::changes(&h.storage).is_empty());
}

#[test]
fn catching_up_folds_the_log_in_and_takes_books_that_came() {
    let h = harness(MemoryStorage::new().with("books/a.txt", "one"));
    let env = h.env();
    walk(&env);
    let index = Index::build(&h.storage, 100, "");
    library::set_finished(&h.storage, "books/a.txt", true);
    h.storage.write("books/new.txt", b"new").unwrap();
    catch_up(&env, &["books/new.txt", "books/gone.txt"]);
    assert_eq!(
        books(&h.storage),
        [
            ("books/a.txt".to_string(), Some(100)),
            ("books/new.txt".to_string(), None)
        ]
    );
    // A new catalog: an index of the old one no longer holds.
    let service = library::service(&env);
    assert!(!service.is_current(&env, &index));
    assert!(service.is_current(&env, &service.index(&env, 100, "")));
}

/// A storage whose appends fail after `left` of them, as a full card's
/// do, and whose reads of `unreadable` fail.
struct Failing<'a> {
    inner: &'a MemoryStorage,
    left: Cell<usize>,
    unreadable: &'static str,
}

impl Storage for Failing<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path == self.unreadable {
            return Err(StorageError::Io);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.left.get() == 0 {
            return Err(StorageError::Full);
        }
        self.left.set(self.left.get() - 1);
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }
}

#[test]
fn an_error_leaves_the_old_catalog_as_it_was() {
    let mut storage = MemoryStorage::new();
    for n in 0..200 {
        storage = storage.with(&format!("books/{n:03}.txt"), "a book");
    }
    let h = harness(storage);
    walk(&h.env());
    let before = h.storage.get(".paperpaw/library/catalog.txt").unwrap();
    // The card fills while the next is made: the old one stays, whole.
    h.storage.write("books/zzz.txt", b"one more book").unwrap();
    let failing = Failing {
        inner: &h.storage,
        left: Cell::new(1),
        unreadable: "",
    };
    let env = h.env_on(&failing);
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    let result = loop {
        match walk.step(&env) {
            Ok(true) => break Ok(()),
            Ok(false) => {}
            Err(e) => break Err(e),
        }
    };
    assert_eq!(result, Err(StorageError::Full));
    assert_eq!(
        h.storage.get(".paperpaw/library/catalog.txt").unwrap(),
        before
    );
    assert!(!h.storage.exists(".paperpaw/library/catalog.tmp"));
    // The catalog unreadable: no walk begins, nothing is changed.
    let failing = Failing {
        inner: &h.storage,
        left: Cell::new(usize::MAX),
        unreadable: ".paperpaw/library/catalog.txt",
    };
    let env = h.env_on(&failing);
    assert!(Walk::start(&env, Vec::new()).is_err());
    assert!(Rewrite::start(&env, Vec::new()).is_ok_and(|r| {
        r.abort(&failing);
        true
    }));
    assert_eq!(
        h.storage.get(".paperpaw/library/catalog.txt").unwrap(),
        before
    );
}

#[test]
fn a_status_that_cannot_be_read_is_not_noted() {
    let h = harness(MemoryStorage::new().with("books/a.txt", "one"));
    walk(&h.env());
    let failing = Failing {
        inner: &h.storage,
        left: Cell::new(usize::MAX),
        unreadable: ".paperpaw/library/favourites.txt",
    };
    h.storage
        .write(".paperpaw/library/favourites.txt", b"books/a.txt\n")
        .unwrap();
    library::note_status(&failing, "books/a.txt");
    assert!(catalog::changes(&h.storage).is_empty(), "nothing noted");
    // Readable: noted as the lists say.
    library::note_status(&h.storage, "books/a.txt");
    let noted = catalog::changes(&h.storage);
    let status = noted.status(paperpaw_sdk::digest::fnv1a64("books/a.txt"));
    assert_eq!(status, Some(Status::new(None, true, false)));
}

/// The old catalog has the books of a folder Y before those of X, and
/// `removed` of Y's are removed since: a walk finds its place again past
/// them, and takes every book of X from the old catalog.
fn taken_after_removed(removed: usize) -> (usize, usize) {
    let storage = MemoryStorage::new();
    storage.create_dir("books").unwrap();
    for (dir, n) in [("X", 40), ("Y", removed)] {
        storage.create_dir(&format!("books/{dir}")).unwrap();
        for i in 0..n {
            storage
                .write(&format!("books/{dir}/b{i:03}.txt"), b"text")
                .unwrap();
        }
    }
    let h = harness(MemoryStorage::new());
    let env = h.env_on(&storage);
    walk(&env);
    // Marked, to tell what the next walk takes from the old catalog.
    let mut records = Vec::new();
    catalog::each(&storage, &mut |r| records.push(r));
    for record in &mut records {
        record.title = String::from("OLD");
    }
    catalog::write(&storage, &records).unwrap();
    for i in 0..removed {
        storage.remove(&format!("books/Y/b{i:03}.txt")).unwrap();
    }
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    while !walk.step(&env).unwrap() {}
    let (mut old, mut all) = (0, 0);
    catalog::each(&storage, &mut |r| {
        all += 1;
        old += usize::from(r.title == "OLD");
    });
    assert_eq!(walk.from_old(), old);
    (old, all)
}

#[test]
fn a_walk_finds_its_place_again_past_books_removed() {
    for removed in [10, 31, 33, 100] {
        assert_eq!(taken_after_removed(removed), (40, 40), "{removed} removed");
    }
}

/// Paused after every step (letting go of what it read), a walk makes
/// the same catalog.
#[test]
fn a_walk_paused_goes_on_where_it_was() {
    let mut storage = MemoryStorage::new();
    for n in 0..30 {
        storage = storage.with(&format!("books/Shelf {}/b{n:02}.txt", n % 3), "text");
    }
    let h = harness(storage);
    let env = h.env();
    walk(&env);
    let made = books(&h.storage);
    library::set_finished(&h.storage, "books/Shelf 1/b04.txt", true);
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    loop {
        walk.pause();
        if walk.step(&env).unwrap() {
            break;
        }
    }
    assert_eq!(walk.from_old(), 30);
    let again = books(&h.storage);
    assert_eq!(again.len(), made.len());
    assert!(again.contains(&("books/Shelf 1/b04.txt".to_string(), Some(100))));
}

#[test]
fn a_walk_paused_after_a_new_book_still_takes_the_others_from_the_old_catalog() {
    // A book new to the library anywhere among nine known, the walk paused
    // after every step: the nine still come from the old catalog.
    for new in ["a-new.txt", "b05-new.txt", "z-new.txt"] {
        let storage = MemoryStorage::new();
        storage.create_dir("books").unwrap();
        for n in 0..9 {
            storage
                .write(&format!("books/b{n:02}.txt"), b"text")
                .unwrap();
        }
        let h = harness(MemoryStorage::new());
        let env = h.env_on(&storage);
        walk(&env);
        storage.write(&format!("books/{new}"), b"text").unwrap();
        let mut walk = Walk::start(&env, Vec::new()).unwrap();
        loop {
            walk.pause();
            if walk.step(&env).unwrap() {
                break;
            }
        }
        assert_eq!(walk.from_old(), 9, "{new}");
        assert_eq!(walk.found(), 10, "{new}");
    }
}

#[test]
fn a_line_too_long_in_the_old_catalog_is_passed_over() {
    let storage = MemoryStorage::new();
    storage.create_dir("books").unwrap();
    for n in 0..5 {
        storage
            .write(&format!("books/b{n:02}.txt"), b"text")
            .unwrap();
    }
    let h = harness(MemoryStorage::new());
    let env = h.env_on(&storage);
    walk(&env);
    // A line of 100 KB (not one the catalog writes) before the third book.
    let path = ".paperpaw/library/catalog.txt";
    let text = paperpaw_sdk::read_file(&storage, path, 1 << 20).unwrap();
    let text = String::from_utf8(text).unwrap();
    let at = text.find("books/b02.txt").unwrap();
    let long = format!("{}{}\n{}", &text[..at], "x".repeat(100 * 1024), &text[at..]);
    storage.write(path, long.as_bytes()).unwrap();
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    while !walk.step(&env).unwrap() {}
    assert_eq!(walk.from_old(), 5);
    assert_eq!(walk.found(), 5);
}

#[test]
fn the_kernels_library_deletes_books_and_says_which_are_gone() {
    let storage = MemoryStorage::new()
        .with("books/a.txt", "a")
        .with("books/b.txt", "b");
    let h = harness(MemoryStorage::new());
    let env = h.env_on(&storage);
    let library = library::service(&env);
    assert!(library.can_delete(&env, "books/a.txt"));
    assert!(!library.can_delete(&env, "books/gone.txt"));
    let deleted = library.delete(&env, &["books/a.txt", "books/gone.txt", "books/b.txt"]);
    assert!(!storage.exists("books/a.txt") && !storage.exists("books/b.txt"));
    // One gone already is forgotten as well; none failed.
    assert!(deleted.failed.is_empty());
    // Each one gone said so: the kernel then forgets it and what is kept.
    let removed: Vec<String> = deleted
        .commands
        .iter()
        .filter_map(|command| match command {
            paperpaw_sdk::Command::Emit(id, params) => paperpaw_sdk::builtin::events::BOOK_REMOVED
                .read(id, params)
                .map(|r| r.path),
            _ => None,
        })
        .collect();
    assert_eq!(removed, ["books/a.txt", "books/gone.txt", "books/b.txt"]);
}

/// A card that cannot be written: what is there reads, nothing goes.
struct ReadOnly(MemoryStorage);

impl Storage for ReadOnly {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, at, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

#[test]
fn only_books_are_deleted_and_what_fails_says_why() {
    let storage = MemoryStorage::new()
        .with("books/a.txt", "a")
        .with("books/notes.md", "not of a format known")
        .with("paperpaw.ini", "[core]")
        .with(".paperpaw/library/x.txt", "kept");
    let h = harness(MemoryStorage::new());
    let env = h.env_on(&storage);
    let library = library::service(&env);
    // Not a book: of no format known, in PaperPaw's own folder (also by
    // another spelling, or climbing up to it).
    for path in [
        "books/notes.md",
        "paperpaw.ini",
        ".paperpaw/library/x.txt",
        ".PaperPaw/library/x.txt",
        "books/../.paperpaw/library/x.txt",
    ] {
        assert!(!library.can_delete(&env, path), "{path}");
        let deleted = library.delete(&env, &[path]);
        assert!(
            deleted.commands.is_empty() && deleted.failed.is_empty(),
            "{path}"
        );
        assert_eq!(deleted.refused, [path]);
    }
    assert!(storage.exists("books/notes.md") && storage.exists(".paperpaw/library/x.txt"));
    // A card that cannot be written: the book stays, and why is said.
    let locked = ReadOnly(MemoryStorage::new().with("books/a.txt", "a"));
    let env = h.env_on(&locked);
    let deleted = library::service(&env).delete(&env, &["books/a.txt"]);
    assert!(deleted.commands.is_empty());
    assert_eq!(
        deleted.failed,
        [("books/a.txt".to_string(), StorageError::Unsupported)]
    );
    assert!(locked.exists("books/a.txt"));
}

#[test]
fn a_book_without_an_extension_known_by_its_first_bytes_is_deleted() {
    let storage = MemoryStorage::new()
        .with("books/Story", "%STORY and the rest of it")
        .with("books/README", "plain words, of no format");
    let mut h = harness(MemoryStorage::new());
    let mut formats = Formats::new();
    formats
        .add(
            FormatSpec::new(
                Id::new_static("test.story"),
                "Story",
                vec![Cow::Borrowed("story")],
                Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
            )
            .sniff(|bytes| bytes.starts_with(b"%STORY")),
        )
        .unwrap();
    h.formats = formats;
    let env = h.env_on(&storage);
    let library = library::service(&env);
    assert!(library.can_delete(&env, "books/Story"));
    assert!(!library.can_delete(&env, "books/README"));
    let deleted = library.delete(&env, &["books/Story", "books/README"]);
    assert!(!storage.exists("books/Story"));
    assert!(storage.exists("books/README"));
    assert_eq!(deleted.refused, ["books/README"]);
    assert_eq!(deleted.commands.len(), 1);
}

/// A storage that tells which paths were written (written, appended to,
/// removed, renamed, made).
struct Watched<'a> {
    inner: &'a MemoryStorage,
    written: std::cell::RefCell<Vec<String>>,
}

impl<'a> Watched<'a> {
    fn new(inner: &'a MemoryStorage) -> Self {
        Watched {
            inner,
            written: Default::default(),
        }
    }

    fn noted(&self, path: &str) {
        self.written.borrow_mut().push(path.to_string());
    }

    /// The paths written since asked last, each once, sorted.
    fn taken(&self) -> Vec<String> {
        let mut written = self.written.take();
        written.sort();
        written.dedup();
        written
    }
}

impl Storage for Watched<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.noted(path);
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.noted(path);
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_from(dir, from, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.noted(path);
        self.inner.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.noted(from);
        self.noted(to);
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.noted(path);
        self.inner.create_dir(path)
    }
}

const CATALOG: &str = ".paperpaw/library/catalog.txt";
const LOG: &str = ".paperpaw/library/status.log";

/// A library of `n` books in three folders, its catalog made.
fn library_of(n: usize) -> common::Harness {
    let mut storage = MemoryStorage::new();
    for i in 0..n {
        let shelf = ["Alpha", "Beta", "Gamma"][i % 3];
        storage = storage.with(&format!("books/{shelf}/book number {i:04}.txt"), "text");
    }
    let h = harness(storage);
    walk(&h.env());
    h
}

/// The catalog's lines but its header.
fn lines(storage: &MemoryStorage) -> Vec<String> {
    let text = String::from_utf8(storage.get(CATALOG).unwrap()).unwrap();
    text.lines().skip(1).map(str::to_string).collect()
}

#[test]
fn a_walk_that_finds_nothing_changed_writes_nothing() {
    let h = library_of(300);
    let before = h.storage.get(CATALOG).unwrap();
    let generation = catalog::generation(&h.storage);
    let watched = Watched::new(&h.storage);
    let env = h.env_on(&watched);
    walk(&env);
    assert_eq!(watched.taken(), Vec::<String>::new());
    assert_eq!(h.storage.get(CATALOG).unwrap(), before);
    assert_eq!(catalog::generation(&h.storage), generation);
    // Paused after every step: still nothing.
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    loop {
        walk.pause();
        if walk.step(&env).unwrap() {
            break;
        }
    }
    assert_eq!(walk.found(), 300);
    assert_eq!(watched.taken(), Vec::<String>::new());
}

#[test]
fn a_folder_too_large_to_list_in_one_piece_is_listed_into_a_file() {
    let mut storage = MemoryStorage::new();
    for i in 0..300 {
        storage = storage.with(
            &format!("books/a book with a name long enough to fill a piece {i:04}.txt"),
            "text",
        );
    }
    let h = harness(storage);
    walk(&h.env());
    let before = h.storage.get(CATALOG).unwrap();
    let watched = Watched::new(&h.storage);
    walk(&h.env_on(&watched));
    // Its listing only, gone once it is walked.
    assert_eq!(watched.taken(), [".paperpaw/library/walk.list"]);
    assert!(!h.storage.exists(".paperpaw/library/walk.list"));
    assert_eq!(h.storage.get(CATALOG).unwrap(), before);
    assert_eq!(books(&h.storage).len(), 300);
}

#[test]
fn a_walk_that_finds_a_book_changed_writes_the_catalog_with_it() {
    for paused in [false, true] {
        let h = library_of(300);
        let before = lines(&h.storage);
        let generation = catalog::generation(&h.storage).unwrap();
        // In the second folder walked, past the first piece copied.
        let changed = "books/Beta/book number 0151.txt";
        h.storage.write(changed, b"longer text").unwrap();
        let env = h.env();
        let mut walk = Walk::start(&env, Vec::new()).unwrap();
        loop {
            if paused {
                walk.pause();
            }
            if walk.step(&env).unwrap() {
                break;
            }
        }
        assert_eq!(walk.found(), 300);
        assert_eq!(catalog::generation(&h.storage), Some(generation + 1));
        let after = lines(&h.storage);
        assert_eq!(after.len(), before.len());
        let differ: Vec<&String> = after
            .iter()
            .zip(&before)
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a)
            .collect();
        assert_eq!(differ.len(), 1, "paused {paused}");
        assert!(differ[0].starts_with(changed));
        assert!(!h.storage.exists(".paperpaw/library/catalog.tmp"));
    }
}

#[test]
fn a_walk_that_finds_books_gone_or_come_at_the_end_writes_the_catalog() {
    let h = library_of(300);
    let before = lines(&h.storage);
    // The folder walked last (the first listed: folders are taken from
    // the end) gone: the old catalog's last lines.
    let last = before.last().unwrap().split('\t').next().unwrap();
    let shelf = last.rsplit_once('/').unwrap().0.to_string();
    h.storage.remove_dir_all(&shelf).unwrap();
    walk(&h.env());
    let after = lines(&h.storage);
    assert_eq!(after.len(), 200);
    assert_eq!(after[..], before[..200]);
    // One come at the end of a folder walked last.
    h.storage
        .write(
            &format!("{}/zzz.txt", before[199].rsplit_once('/').unwrap().0),
            b"new",
        )
        .unwrap();
    walk(&h.env());
    let again = lines(&h.storage);
    assert_eq!(again.len(), 201);
    assert_eq!(again[..200], after[..]);
}

#[test]
fn a_walk_keeps_the_log_where_the_catalog_has_what_it_noted() {
    let h = library_of(30);
    let book = "books/Beta/book number 0010.txt";
    let before = h.storage.get(CATALOG).unwrap();
    let generation = catalog::generation(&h.storage).unwrap();
    // What the catalog has noted again: the log stays as it is.
    catalog::note(&h.storage, book, Status::new(None, false, false)).unwrap();
    let log = h.storage.get(LOG).unwrap();
    let watched = Watched::new(&h.storage);
    walk(&h.env_on(&watched));
    assert_eq!(watched.taken(), Vec::<String>::new());
    assert_eq!(h.storage.get(LOG).unwrap(), log);
    // A book noted removed that is there: its lines as they were, but a
    // catalog anew (views make theirs again) and the log cut (it would
    // hide the book).
    catalog::forget(&h.storage, book).unwrap();
    assert!(!books(&h.storage).iter().any(|(p, _)| p == book));
    walk(&h.env_on(&watched));
    assert_eq!(
        watched.taken(),
        [".paperpaw/library/catalog.tmp", CATALOG, LOG]
    );
    assert_eq!(catalog::generation(&h.storage), Some(generation + 1));
    assert_eq!(lines(&h.storage), {
        let text = String::from_utf8(before).unwrap();
        text.lines().skip(1).map(str::to_string).collect::<Vec<_>>()
    });
    assert!(catalog::changes(&h.storage).is_empty());
    assert!(books(&h.storage).iter().any(|(p, _)| p == book));
}

/// How far a walk is: never back, below 1,000,000 until it is done, then
/// 1,000,000, on a deep and a wide tree of folders.
#[test]
fn a_walks_progress_never_goes_back_and_ends_at_a_million() {
    let deep = (0..12).fold(String::from("books"), |path, n| format!("{path}/d{n}"));
    let mut trees = Vec::new();
    let mut storage = MemoryStorage::new();
    let mut path = String::from("books");
    for n in 0..12 {
        path = format!("{path}/d{n}");
        storage = storage.with(&format!("{path}/b.txt"), "a book");
    }
    assert!(path == deep);
    trees.push(storage);
    let mut storage = MemoryStorage::new();
    for n in 0..40 {
        storage = storage.with(&format!("books/w{n:02}/b.txt"), "a book");
        storage = storage.with(&format!("books/w{n:02}/x/c.txt"), "a book");
    }
    trees.push(storage);
    for storage in trees {
        let h = harness(storage);
        let env = h.env();
        let mut walk = Walk::start(&env, Vec::new()).unwrap();
        assert_eq!(walk.progress_ppm(), 0);
        let mut said = Vec::new();
        while !walk.step(&env).unwrap() {
            said.push(walk.progress_ppm());
        }
        assert!(said.windows(2).all(|w| w[0] <= w[1]), "went back: {said:?}");
        assert!(said.iter().all(|ppm| *ppm < 1_000_000));
        assert!(said.iter().any(|ppm| *ppm > 0 && *ppm < 1_000_000));
        assert_eq!(walk.progress_ppm(), 1_000_000);
    }
}

/// A library with a native format's book (the kernel's stand-in answers
/// "not ready") beside an interface 6 one, the device knowing the date.
fn native_library() -> common::Harness {
    let storage = MemoryStorage::new()
        .with("books/a.docx", "PK native")
        .with("books/b.txt", "text");
    let mut h = harness(storage).with_clock(1_760_000_000_000, Some(0));
    h.formats
        .add(FormatSpec::new(
            Id::new_static("native.docx"),
            "DOCX",
            vec![Cow::Borrowed("docx")],
            Box::new(|_, _, _| Err(DocumentError::NotReady)),
        ))
        .unwrap();
    h
}

fn title_of(storage: &dyn Storage, path: &str) -> (String, String, Option<u32>) {
    let mut found = None;
    catalog::each(storage, &mut |r| {
        if r.path == path {
            found = Some((r.title.clone(), r.author.clone(), r.added));
        }
    });
    found.unwrap()
}

#[test]
fn a_native_book_is_read_by_the_host_later_and_an_unchanged_walk_writes_nothing() {
    let h = native_library();
    walk(&h.env());
    // Nothing read of it: its file's name, and not marked as read.
    let (title, author, added) = title_of(&h.storage, "books/a.docx");
    assert_eq!((title.as_str(), author.as_str()), ("a", ""));
    assert!(added.is_some(), "the day it came is kept");
    // Walked again before the host read it: the same line, nothing written.
    let before = h.storage.get(CATALOG).unwrap();
    let watched = Watched::new(&h.storage);
    walk(&h.env_on(&watched));
    assert_eq!(watched.taken(), Vec::<String>::new());
    assert_eq!(h.storage.get(CATALOG).unwrap(), before);
    // The view's index does not mark it either.
    let book = index(&h.env(), "books/a.docx");
    assert!(!book.indexed);

    // The host's details job read it through its format plugin.
    let mut metadata = paperpaw_sdk::document::Metadata::default();
    metadata.title = Some("The Real Title".into());
    metadata.authors = vec!["Ann Author".into(), "Ben Second".into()];
    metadata.series = Some("Saga".into());
    metadata.series_index = Some("2".into());
    metadata.subjects = vec!["fiction".into()];
    library::keep_native_details(&h.env(), "books/a.docx", &metadata).unwrap();
    walk(&h.env());
    let (title, author, again) = title_of(&h.storage, "books/a.docx");
    assert_eq!(
        (title.as_str(), author.as_str()),
        ("The Real Title", "Ann Author")
    );
    assert_eq!(again, added, "the day it came stays");
    // And from then on an unchanged walk writes nothing.
    let before = h.storage.get(CATALOG).unwrap();
    let watched = Watched::new(&h.storage);
    walk(&h.env_on(&watched));
    assert_eq!(watched.taken(), Vec::<String>::new());
    assert_eq!(h.storage.get(CATALOG).unwrap(), before);
    assert_eq!(books(&h.storage).len(), 2);
}

#[test]
fn native_details_of_a_book_that_is_gone_are_not_kept() {
    let h = native_library();
    walk(&h.env());
    let metadata = paperpaw_sdk::document::Metadata::default();
    assert!(library::keep_native_details(&h.env(), "books/gone.docx", &metadata).is_err());
}
