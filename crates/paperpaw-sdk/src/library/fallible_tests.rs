//! A memory shortage while a book's details or status are read is the
//! error, and the details file is as it was: every allocation of the
//! read refused in turn.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;
use super::*;
use crate::native::commands::menu_alloc_tests::refuse_nth;
use paperpaw_platform::DirEntry;
use std::cell::RefCell;
use std::string::ToString;
use std::vec;

const BOOK: &str = "books/a.epub";

/// A card that is read without allocating; writes are noted.
struct Card {
    files: RefCell<Vec<(String, Vec<u8>)>>,
    writes: RefCell<usize>,
}

impl Card {
    fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        // Outside the sweep: only for the test's own checks.
        self.files
            .borrow()
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, data)| data.clone())
            .ok_or(StorageError::NotFound)
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let files = self.files.borrow();
        let data = files
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, data)| data)
            .ok_or(StorageError::NotFound)?;
        let from = usize::try_from(offset).unwrap().min(data.len());
        let n = buf.len().min(data.len() - from);
        buf[..n].copy_from_slice(&data[from..from + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let files = self.files.borrow();
        files
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, data)| data.len() as u64)
            .ok_or(StorageError::NotFound)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        *self.writes.borrow_mut() += 1;
        Ok(())
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        *self.writes.borrow_mut() += 1;
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        *self.writes.borrow_mut() += 1;
        Ok(())
    }
}

const DETAILS: &str = "[book]\nsize = 5\nversion = 4\ntitle = \"A \\\"quoted\\\" title\"\n\
author = Ada\nprogress = 40\ntag.0 = fiction\n[field]\nmyplugin.rating = 5\n";

fn card() -> Card {
    Card {
        files: RefCell::new(vec![
            (BOOK.to_string(), vec![0; 5]),
            (book_file(BOOK), DETAILS.as_bytes().to_vec()),
            (favourites_file(), b"books/a.epub\n".to_vec()),
            (opened_file(), b"books/a.epub\n".to_vec()),
        ]),
        writes: RefCell::new(0),
    }
}

/// `read` with each of its allocations refused in turn: every refusal is
/// `OutOfMemory`, no file is written and the details are byte for byte
/// as they were; the first call it does not reach is `Ok`. Returns how
/// many allocations there are.
fn sweep<T: core::fmt::Debug>(
    card: &Card,
    read: impl Fn(&Card) -> Result<T, StorageError>,
) -> usize {
    let before = card.get(&book_file(BOOK)).unwrap();
    for (refusals, number) in (1..).enumerate() {
        let (result, refused) = refuse_nth(number, || read(card));
        assert_eq!(
            card.get(&book_file(BOOK)).unwrap(),
            before,
            "refusal {number}"
        );
        assert_eq!(*card.writes.borrow(), 0, "refusal {number} wrote");
        if !refused {
            result.unwrap();
            return refusals;
        }
        assert!(
            matches!(result, Err(StorageError::OutOfMemory)),
            "refusal {number}: {result:?}"
        );
    }
    unreachable!()
}

#[test]
fn the_book_to_update_is_read_whole_or_refused_with_the_details_untouched() {
    let card = card();
    let book = book_for_update(&card, BOOK).unwrap();
    assert_eq!((book.progress, book.tags.len()), (Some(40), 1));
    assert!(sweep(&card, |card| book_for_update(card, BOOK)) >= 5);
}

#[test]
fn the_status_is_read_whole_or_refused_with_the_details_untouched() {
    let card = card();
    let status = status_of(&card, BOOK).unwrap();
    assert_eq!(
        (status.progress, status.favourite, status.opened),
        (Some(40), true, true)
    );
    assert!(sweep(&card, |card| status_of(card, BOOK)) >= 3);
}

#[test]
fn the_fallible_file_names_are_the_catalogs() {
    use super::catalog::{catalog_path, log_path};
    // The catalog reads name their files without an infallible
    // allocation; the names must stay those of `catalog_path`/`log_path`.
    assert_eq!(crate::library::catalog::try_count(&card()), Ok(0));
    let (catalog, log) = (catalog_path(), log_path());
    assert!(catalog.ends_with("/catalog.txt") && log.ends_with("/status.log"));
    assert!(catalog.starts_with(DIR) && log.starts_with(DIR));
}

#[test]
fn a_file_the_heap_cannot_hold_is_out_of_memory_not_too_large() {
    let card = card();
    let file = book_file(BOOK);
    let (refused, hit) = refuse_nth(1, || crate::read_file(&card, &file, 1 << 20));
    assert!(hit);
    assert_eq!(refused, Err(StorageError::OutOfMemory));
    // Too large stays the file's own size.
    assert_eq!(
        crate::read_file(&card, &file, 3),
        Err(StorageError::TooLarge)
    );
}

/// A card with `details` as the book's details file.
fn card_with(details: &str) -> Card {
    let card = card();
    card.files.borrow_mut()[1].1 = details.as_bytes().to_vec();
    card
}

#[test]
fn indexed_is_what_the_whole_details_say_without_reading_them() {
    let long_field = "x".repeat(300);
    let cases: [(&str, bool); 10] = [
        (
            "[book]\nsize = 5\nindexed = on\nversion = 4\ntitle = T\n",
            true,
        ),
        ("[book]\nsize = 5\nindexed = off\nversion = 4\n", false),
        ("[book]\nsize = 5\nindexed = on\nversion = 3\n", false),
        ("[book]\nsize = 6\nindexed = on\nversion = 4\n", false),
        // A field called "indexed" is a field, not the key.
        (
            "[book]\nsize = 5\nversion = 4\n[field]\nindexed = on\n",
            false,
        ),
        (
            "# format 2\n[book]\nsize = 5\nindexed = on\nversion = 4\n",
            false,
        ),
        (
            "[book]\r\nsize = 5\r\nindexed = yes\r\nversion = 4\r\n",
            true,
        ),
        ("\u{feff}[book]\nsize = 5\nindexed = on\nversion = 4", true),
        ("", false),
        // A long line before the keys is skipped, not held.
        (
            &format!("[book]\ntitle = {long_field}\nsize = 5\nindexed = on\nversion = 4\n"),
            true,
        ),
    ];
    for (details, expected) in cases {
        let card = card_with(details);
        assert_eq!(try_indexed(&card, BOOK, 5), Ok(expected), "{details:?}");
        // The same as the whole read says.
        let whole = try_book_sized(&card, BOOK, 5).ok().flatten();
        assert_eq!(whole.is_some_and(|b| b.indexed), expected, "{details:?}");
    }
    // No details; details of 17 KiB (refused as the whole read refuses them).
    let card = card_with("");
    card.files.borrow_mut().remove(1);
    assert_eq!(try_indexed(&card, BOOK, 5), Ok(false));
    let big = format!(
        "[book]\nsize = 5\nindexed = on\nversion = 4\n{}",
        "#".repeat(17 * 1024)
    );
    assert_eq!(try_indexed(&card_with(&big), BOOK, 5), Ok(false));
}

#[test]
fn indexed_is_read_or_refused_for_every_allocation() {
    let details = "[book]\nsize = 5\nindexed = on\nversion = 4\n";
    let card = card_with(details);
    let mut allocations = 0;
    for number in 1.. {
        let (result, refused) = refuse_nth(number, || try_indexed(&card, BOOK, 5));
        if !refused {
            assert_eq!(result, Ok(true));
            allocations = number - 1;
            break;
        }
        assert_eq!(
            result,
            Err(crate::data::Error::OutOfMemory),
            "refusal {number}"
        );
    }
    assert!(allocations >= 1);
    assert_eq!(*card.writes.borrow(), 0);
}
