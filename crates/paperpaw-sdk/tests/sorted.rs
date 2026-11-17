//! The library's books sorted on the card ([`library::sorted`]): made as
//! the kernel makes them, a view pages them as the index lists them (every
//! filter, the books read last first or by title, series stacked, the next
//! book of each series), and a file of another version, cut short or
//! damaged is never read; a making cut short leaves the file there was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use paperpaw_sdk::library::catalog::{self, Record, Status};
use paperpaw_sdk::library::index::{self};
use paperpaw_sdk::library::sorted::Build;

use paperpaw_sdk::library::Book;
use paperpaw_sdk::{DirEntry, Env, MemoryStorage, Storage, StorageError};
use std::cell::Cell;

/// The file of the sorted books for a view leaving `archive` out.
fn file_of(archive: &str) -> String {
    format!(".paperpaw/library/sorted-{:08x}.idx", index::hash(archive))
}

/// The file of the view most tests make.
fn file() -> String {
    file_of("books/Archive")
}

/// Titles in several scripts, articles and cases, alike at their start.
const TITLES: [&str; 12] = [
    "The Hound of the Baskervilles",
    "a Study in Scarlet",
    "Émile",
    "Zola",
    "كتاب الرمل",
    "한국어 책",
    "Das Schloss",
    "das Urteil",
    "1984",
    "Book",
    "Book",
    "",
];

/// A library of `n` books: series of one to five books (some without),
/// some being read, finished or favourites, in folders and the archive.
fn library(n: usize) -> MemoryStorage {
    let storage = MemoryStorage::new();
    let mut records = Vec::new();
    for i in 0..n {
        let folder = if i % 17 == 0 { "Archive" } else { "Shelf" };
        let path = format!("books/{folder}/b{i:05}.epub");
        let series = if i % 7 == 0 {
            String::new()
        } else {
            format!("Series {}", (i * 13) % (n / 3 + 1))
        };
        let title = format!("{} {}", TITLES[i % TITLES.len()], (i * 7919) % 97);
        let book = Book::default()
            .title(Some(title))
            .author(Some(format!("Author {}", i % 31)))
            .series((!series.is_empty()).then_some(series))
            .series_index(Some(format!("{}", i % 5 + 1)))
            .size(100 + i as u64);
        let progress = match i % 9 {
            0 => Some(100),
            1 | 2 => Some((i % 98 + 1) as u8),
            _ => None,
        };
        let status = Status::new(progress, i % 11 == 0, progress.is_some() || i % 13 == 0);
        records.push(Record::of(&path, &book).with_status(status));
    }
    catalog::write(&storage, &records).unwrap();
    // Read last: some of every kind.
    let opened: Vec<String> = (0..n)
        .filter(|i| i % 23 == 0 || i % 9 == 1)
        .take(60)
        .map(|i| format!("books/Shelf/b{i:05}.epub"))
        .collect();
    storage
        .write(".paperpaw/library/opened.txt", opened.join("\n").as_bytes())
        .unwrap();
    // Noted since the catalog was made.
    for i in (5..n).step_by(41) {
        let path = format!("books/Shelf/b{i:05}.epub");
        catalog::note(&storage, &path, Status::new(Some(50), true, true)).unwrap();
    }
    storage
}

/// Makes the file to its end, pausing every `pause` steps.
fn make(env: &Env<'_>, archive: &str, pause: Option<usize>) -> usize {
    let mut build = Build::start(env, archive).unwrap();
    let mut steps = 0;
    while !build.step(env).unwrap() {
        steps += 1;
        if pause.is_some_and(|p| steps % p == 0) {
            build.pause(env.storage());
        }
        assert!(steps < 100_000, "never done");
    }
    steps
}

#[test]
fn a_making_paused_makes_the_same_file() {
    let h = harness(library(700));
    let env = h.env();
    make(&env, "books/Archive", None);
    let whole = h.storage.get(&file()).unwrap();
    for pause in [1, 3, 20] {
        h.storage.remove(&file()).unwrap();
        make(&env, "books/Archive", Some(pause));
        assert_eq!(
            h.storage.get(&file()).unwrap(),
            whole,
            "paused every {pause}"
        );
    }
    assert!(!h.storage.exists(".paperpaw/library/sorted.runs"));
    assert!(!h.storage.exists(".paperpaw/library/sorted.tmp"));
}

/// A storage whose append number `fail_at` (from 1) fails, as a full
/// card's does.
struct FailingOnce<'a> {
    inner: &'a MemoryStorage,
    appends: Cell<usize>,
    fail_at: Cell<usize>,
}

impl Storage for FailingOnce<'_> {
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
        self.appends.set(self.appends.get() + 1);
        if self.appends.get() == self.fail_at.get() {
            return Err(StorageError::Full);
        }
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

/// A pause whose run is written in part only (the card full after its
/// first piece) leaves no file of books shifted: the making fails, and
/// the next one makes the file whole.
#[test]
fn a_pause_written_in_part_makes_no_file() {
    let h = harness(library(700));
    make(&h.env(), "books/Archive", None);
    let whole = h.storage.get(&file()).unwrap();
    h.storage.remove(&file()).unwrap();
    let failing = FailingOnce {
        inner: &h.storage,
        appends: Cell::new(0),
        fail_at: Cell::new(0),
    };
    let env = h.env_on(&failing);
    let mut build = Build::start(&env, "books/Archive").unwrap();
    // Three steps: some 190 books held, written in three pieces.
    for _ in 0..3 {
        assert!(!build.step(&env).unwrap());
    }
    failing.fail_at.set(failing.appends.get() + 2);
    build.pause(&failing);
    let mut failed = false;
    for _ in 0..100_000 {
        match build.step(&env) {
            Ok(true) => break,
            Ok(false) => {}
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert!(failed, "made from shifted runs");
    assert!(!h.storage.exists(&file()));
    assert!(!h.storage.exists(".paperpaw/library/sorted.runs"));
    make(&env, "books/Archive", None);
    assert_eq!(h.storage.get(&file()).unwrap(), whole);
}

fn harness(storage: MemoryStorage) -> common::Harness {
    let mut h = common::Harness::new();
    h.storage = storage;
    h
}
