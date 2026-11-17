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
use paperpaw_sdk::library::{self};
use paperpaw_sdk::{DirEntry, Env, FormatSpec, Formats, Id, MemoryStorage, Storage, StorageError};

fn harness(storage: MemoryStorage) -> common::Harness {
    let mut h = common::Harness::new();
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.txt"),
            "TXT",
            vec![Cow::Borrowed("txt")],
            |_, _, _| Err(DocumentError::UnsupportedFormat),
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
            |_, _, _| Err(DocumentError::NotReady),
        ))
        .unwrap();
    h
}

#[test]
fn native_details_of_a_book_that_is_gone_are_not_kept() {
    let h = native_library();
    walk(&h.env());
    let metadata = paperpaw_sdk::document::Metadata::default();
    assert!(library::keep_native_details(&h.env(), "books/gone.docx", &metadata).is_err());
}
