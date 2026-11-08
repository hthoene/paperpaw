//! Memory refused while the library reads one book's details is an error
//! ([`Error::OutOfMemory`]), never an abort and never a half-read book:
//! the global allocator here refuses the n-th allocation of a call (per
//! thread, undone on drop), for every n up to the call's last.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_sdk::data::Error;
use paperpaw_sdk::library::{self, KEY_LEN};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

thread_local! {
    static UNTIL: Cell<Option<usize>> = const { Cell::new(None) };
    static REFUSED: Cell<bool> = const { Cell::new(false) };
}

fn refuse() -> bool {
    UNTIL
        .try_with(|left| match left.get() {
            Some(1) => {
                left.set(None);
                REFUSED.with(|refused| refused.set(true));
                true
            }
            Some(n) => {
                left.set(Some(n - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}

struct Refusing;

// SAFETY: forwards to the system allocator, or refuses with null, which
// `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by `System` with this layout.
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: allocated by `System` with this layout.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Runs `operation` with its `number`-th allocation refused; whether one
/// was refused comes back with the result.
fn with_refusal<T>(number: usize, operation: impl FnOnce() -> T) -> (T, bool) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            UNTIL.with(|left| left.set(None));
        }
    }
    REFUSED.with(|refused| refused.set(false));
    UNTIL.with(|left| left.set(Some(number)));
    let reset = Reset;
    let result = operation();
    drop(reset);
    (result, REFUSED.with(Cell::get))
}

/// Files that are read without allocating.
struct Files(Vec<(String, Vec<u8>)>);

impl Files {
    fn get(&self, path: &str) -> Result<&[u8], StorageError> {
        self.0
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, data)| data.as_slice())
            .ok_or(StorageError::NotFound)
    }
}

impl Storage for Files {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let data = self.get(path)?;
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - from);
        buf[..n].copy_from_slice(&data[from..from + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.get(path).map(|data| data.len() as u64)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

const BOOK: &str = "books/a.epub";
const DETAILS: &str = "\
# format 1
[book]
size = 5
indexed = on
version = 4
title = \"A \\\"quoted\\\" title\\n\"
author = Ada Author
series = The Series
series-index = 2
tag.0 = fiction
tag.1 = \"two words\"
tag.2 = \"esc\\tape\"
cover = covers/a.png
cover-width = 300
cover-height = 450
progress = 40
added = 20000
[field]
myplugin.rating = 5
myplugin.note = \"a \\\"note\\\"\"
";

fn files() -> Files {
    let mut digits = [0_u8; KEY_LEN];
    let key = library::key_into(BOOK, &mut digits);
    let details = format!(".paperpaw/library/details/{}/{key}.ini", &key[..2]);
    Files(vec![
        (BOOK.to_owned(), vec![0; 5]),
        // The mark line is part of the first version's files only from 2.
        (
            details,
            DETAILS
                .trim_start_matches("# format 1\n")
                .as_bytes()
                .to_vec(),
        ),
    ])
}

#[test]
fn a_book_is_read_whole_or_refused_for_every_allocation() {
    let storage = files();
    let whole = library::try_book(&storage, BOOK).unwrap().unwrap();
    assert_eq!(whole.title.as_deref(), Some("A \"quoted\" title\n"));
    assert_eq!((whole.tags.len(), whole.fields.len()), (3, 2));
    assert_eq!(whole.cover.as_ref().map(|c| c.height), Some(450));
    let mut refusals = 0;
    for number in 1.. {
        let (result, refused) = with_refusal(number, || library::try_book(&storage, BOOK));
        if !refused {
            assert_eq!(result.unwrap().unwrap(), whole);
            break;
        }
        refusals += 1;
        assert_eq!(result, Err(Error::OutOfMemory), "refusal {number}");
    }
    assert!(refusals >= 10, "only {refusals} allocation sites");
    // The plain form is the same book, or none.
    for number in 1..=refusals {
        let (book, _) = with_refusal(number, || library::book(&storage, BOOK));
        assert_eq!(book, None, "refusal {number}");
    }
}

#[test]
fn a_book_of_another_size_or_none_is_not_a_memory_error() {
    let storage = files();
    assert_eq!(library::try_book_sized(&storage, BOOK, 6), Ok(None));
    assert_eq!(library::try_book(&storage, "books/none.epub"), Ok(None));
}

#[test]
fn the_key_is_the_same_whichever_form_asks() {
    let mut digits = [0_u8; KEY_LEN];
    let key = library::key_into(BOOK, &mut digits);
    assert_eq!(library::key(BOOK), key);
    assert_eq!(library::try_key(BOOK).unwrap(), key);
    let (result, refused) = with_refusal(1, || library::try_key(BOOK));
    assert!(refused && result.is_err());
}
