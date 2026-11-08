//! The merge when the heap refuses: each allocation of a call is refused in
//! turn. An allocation that would abort (an infallible `format!`,
//! `String::from`, `push`) ends the whole test binary; a refused one must
//! come back as [`StorageError::TooLarge`], with the statistics here as they
//! were and no working file left.
//!
//! Not covered: what `paperpaw-sqlite` allocates while it reads or writes a
//! database (a refusal there aborts), and the 32-digit text of
//! `Md5::hex` for a book without a usable MD5. Merges with no other
//! side's file and no upload do not read or write a database, and books here
//! have MD5s; those are swept.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

use super::tests::{DIR, book, folder, log, md5, stay, this_device};
use super::*;
use crate::testing::heap::{armed, fixture};

/// A storage whose own allocations are not the plugin's: nothing is refused
/// or counted while it works.
#[derive(Default)]
struct Quiet(MemoryStorage);

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fixture(|| self.0.list(dir, visit))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        fixture(|| self.0.rename(from, to))
    }
}

/// The names of the files in `dir` (and the given `extra` paths that exist).
fn names(storage: &Quiet, dir: &str) -> Vec<String> {
    fixture(|| {
        let mut out = Vec::new();
        storage
            .0
            .list(dir, &mut |entry| out.push(String::from(entry.name)))
            .unwrap_or_default();
        out.sort();
        out
    })
}

/// Refuses each allocation of `run` in turn on a fresh storage made by
/// `make`: with none refused it must be `Ok`; with one refused it must be
/// `Err(TooLarge)` or `Ok` (an allocation that is optional); `after` then
/// looks at the storage, with whether the call succeeded.
fn sweep(
    name: &str,
    make: impl Fn() -> Quiet,
    run: impl Fn(&Quiet) -> Result<(), StorageError>,
    after: impl Fn(&Quiet, bool),
) {
    let storage = fixture(&make);
    let (whole, count) = armed(usize::MAX, || run(&storage));
    whole.unwrap_or_else(|error| panic!("{name}: {error:?} with nothing refused"));
    after(&storage, true);
    assert!(
        count > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
    for at in 1..=count {
        let storage = fixture(&make);
        let (result, _) = armed(at, || run(&storage));
        match result {
            Ok(()) => after(&storage, true),
            Err(StorageError::TooLarge) => after(&storage, false),
            Err(error) => panic!("{name}: refusing allocation {at}: {error:?}"),
        }
    }
}

/// `count` records of 8 bytes in a jumbled order.
fn jumbled(count: u32) -> Vec<u8> {
    let mut data = Vec::new();
    let mut x: u32 = 12_345;
    for i in 0..count {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        data.extend_from_slice(&(x % 5000).to_be_bytes());
        data.extend_from_slice(&i.to_be_bytes());
    }
    data
}

fn with_file(path: &str, data: Vec<u8>) -> Quiet {
    let storage = Quiet::default();
    fixture(|| storage.0.write(path, &data)).unwrap();
    storage
}

#[test]
fn sorting_refuses_cleanly_and_leaves_no_run() {
    // 17 runs of 2,048 records: merged in two passes.
    let count = 2048 * 17;
    let input = jumbled(count);
    sweep(
        "sort",
        || with_file("in", input.clone()),
        |s| extsort::sort(s, "in", "out", 8, "tmp"),
        |s, ok| {
            assert_eq!(fixture(|| s.0.get("in")), Some(input.clone()));
            assert!(
                !names(s, "").iter().any(|n| n.starts_with("tmp.")),
                "a run is left"
            );
            if ok {
                let out = fixture(|| s.0.get("out")).unwrap();
                assert_eq!(out.len(), input.len());
                assert!(out.as_chunks::<8>().0.windows(2).all(|w| w[0] <= w[1]));
            }
        },
    );
}

/// The output is the input: a refusal anywhere leaves the input whole (the
/// memory is taken before the file is truncated), and success sorts it.
#[test]
fn sorting_in_place_refuses_cleanly_and_keeps_the_input() {
    for count in [3 * 2048, 2048 * 17] {
        let input = jumbled(count);
        sweep(
            "sort in place",
            || with_file("in", input.clone()),
            |s| extsort::sort(s, "in", "in", 8, "tmp"),
            |s, ok| {
                let now = fixture(|| s.0.get("in")).unwrap();
                assert!(
                    !names(s, "").iter().any(|n| n.starts_with("tmp.")),
                    "a run is left"
                );
                if ok {
                    assert_eq!(now.len(), input.len());
                    assert!(now.as_chunks::<8>().0.windows(2).all(|w| w[0] <= w[1]));
                } else {
                    assert_eq!(now, input, "the input as it was");
                }
            },
        );
    }
}

#[test]
fn a_books_totals_refuse_cleanly_never_partial() {
    use crate::totals::{BookTotals, of_book};
    let make = || device_with(300);
    let storage = fixture(make);
    let one = fixture(|| book(1, &md5(0xa)));
    let whole: BookTotals = fixture(|| of_book(&storage, &one, |u| u as i32 / 500)).unwrap();
    assert!(whole.pages_read > 0 && !whole.days.is_empty());
    sweep(
        "of_book",
        make,
        |s| {
            let got = of_book(s, &one, |u| u as i32 / 500)?;
            assert_eq!(got, whole, "totals that lack stays");
            Ok(())
        },
        |_, _| {},
    );
}

#[test]
fn reading_records_refuses_cleanly() {
    let input = jumbled(300);
    let make = || with_file("in", input.clone());
    sweep(
        "each",
        make,
        |s| {
            let mut n = 0;
            extsort::each(s, "in", 8, |_| {
                n += 1;
                Ok(())
            })?;
            assert_eq!(n, 300);
            Ok(())
        },
        |_, _| {},
    );
    sweep(
        "records",
        make,
        |s| {
            let mut records = extsort::Records::new(s, "in", 8)?;
            assert_eq!(records.by_ref().count(), 300);
            records.failed.map_or(Ok(()), Err)
        },
        |_, _| {},
    );
}

/// Two books here and `stays` in time order.
fn device_with(stays: u32) -> Quiet {
    let storage = Quiet::default();
    fixture(|| {
        this_device(&storage);
        for i in 0..stays {
            store::append(&storage, stay(1 + (i % 2) as u16, i % 90 + 1, 2000 + i, 20)).unwrap();
        }
    });
    storage
}

#[test]
fn a_merge_of_this_device_alone_refuses_cleanly_and_changes_nothing() {
    // Over 819 stays: sorted in more than one run.
    let make = || device_with(1000);
    let storage = fixture(make);
    let (books, stays) = fixture(|| (store::load_books(&storage).unwrap(), log(&storage)));
    sweep(
        "merge",
        make,
        |s| match merge(s, None, None, None, DIR) {
            Ok(_) => Ok(()),
            Err(Failure::Here(Error::Storage(e))) => Err(e),
            Err(other) => panic!("{other}"),
        },
        |s, _| {
            let now = fixture(|| (store::load_books(s).unwrap(), log(s)));
            assert_eq!(now.0.len(), books.len());
            assert_eq!(now.1, stays, "the log as it was");
            assert!(
                !fixture(|| folder(s))
                    .iter()
                    .any(|n| n.starts_with("merge-")),
                "a working file is left"
            );
        },
    );
}

fn known(key: u8, id: i64) -> Known {
    Known {
        key: Key::of(&md5(key), "", ""),
        id,
        notes: 0,
        highlights: 0,
    }
}

fn their_book(id: i64, key: u8, title: &str) -> KoBook {
    KoBook {
        id,
        title: String::from(title),
        authors: String::from("Autorin"),
        md5: md5(key),
        language: String::from("de"),
        series: String::from("Reihe"),
        pages: 200,
        ..KoBook::default()
    }
}

#[test]
fn naming_and_adding_books_refuses_cleanly_and_keeps_the_books() {
    let income = Side {
        path: String::from("income.db"),
        books: alloc::vec![known(0xa, 7), known(0xc, 8), known(0xd, 9)],
        version: koreader::SCHEMA_VERSION,
    };
    let cached = Side {
        path: String::from("cached.db"),
        books: alloc::vec![known(0xa, 7), known(0xb, 6)],
        version: koreader::SCHEMA_VERSION,
    };
    let mut ours = book(1, &md5(0xa));
    ours.language = String::from("N/A");
    let books = alloc::vec![ours, book(2, &md5(0xb))];
    let theirs = alloc::vec![
        their_book(7, 0xa, "Their title"),
        their_book(8, 0xc, "Neu"),
        their_book(9, 0xd, "Auch neu"),
    ];
    let input = RefCell::new(None);
    sweep(
        "merge_books",
        || {
            *input.borrow_mut() = Some((books.clone(), theirs.clone()));
            Quiet::default()
        },
        |_| {
            let (books, theirs) = input.borrow_mut().take().unwrap();
            match merge_books(books, &income, theirs, Some(&cached), 4) {
                Ok((books, gone)) => {
                    assert_eq!(books.len(), 4);
                    assert_eq!(books[0].title, "Their title");
                    assert_eq!(books[0].language, "de");
                    assert_eq!(gone, [(2, false)]);
                    Ok(())
                }
                Err(Failure::Here(Error::Storage(e))) => Err(e),
                Err(other) => panic!("{other}"),
            }
        },
        |_, _| {},
    );
}

#[test]
fn a_book_that_cannot_be_named_keeps_its_names() {
    let theirs = their_book(7, 0xa, "Their title");
    let make = || {
        let mut ours = book(1, &md5(0xa));
        ours.language = String::from("N/A");
        ours
    };
    let ours = RefCell::new(make());
    sweep(
        "take_names",
        || {
            *ours.borrow_mut() = make();
            Quiet::default()
        },
        |_| take_names(&mut ours.borrow_mut(), &theirs),
        |_, ok| {
            let ours = ours.borrow();
            if ok {
                assert_eq!(
                    (ours.title.as_str(), ours.language.as_str()),
                    ("Their title", "de")
                );
            } else {
                assert_eq!(
                    (ours.title.as_str(), ours.language.as_str()),
                    ("Ours 1", "N/A")
                );
            }
        },
    );
}
