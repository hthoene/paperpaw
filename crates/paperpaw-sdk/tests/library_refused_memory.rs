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
use paperpaw_sdk::library::{self, KEY_LEN, catalog};
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

/// The catalog's first line (the header and a generation).
const HEADER: &str = "\tcatalog\t2\t7\n";
const CATALOG: &str = "\
books/a.epub\tAda\tTitle A\t\t\t\t40\tf\t20000\t\t5\n\
books/b.epub\tBob\tTitle B\t\t\t\t\t\t\t\t6\n\
books/c.epub\tCy\tTitle C\t\t\t\t\t\t\t\t7\n";

fn files() -> Files {
    let mut digits = [0_u8; KEY_LEN];
    let key = library::key_into(BOOK, &mut digits);
    let details = format!(".paperpaw/library/details/{}/{key}.ini", &key[..2]);
    let removed = format!("{:016x}\t-\n", paperpaw_digest::fnv1a64("books/b.epub"));
    Files(vec![
        (BOOK.to_owned(), vec![0; 5]),
        (
            ".paperpaw/library/catalog.txt".to_owned(),
            format!("{HEADER}{CATALOG}").into_bytes(),
        ),
        (
            ".paperpaw/library/status.log".to_owned(),
            removed.into_bytes(),
        ),
        (
            ".paperpaw/library/favourites.txt".to_owned(),
            b"books/a.epub\n".to_vec(),
        ),
        (
            ".paperpaw/library/opened.txt".to_owned(),
            b"books/c.epub\r\nbooks/a.epub\n".to_vec(),
        ),
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
    let sites = sweep(&Some(whole), || library::try_book(&storage, BOOK));
    assert!(sites >= 10, "only {sites} allocation sites");
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

/// Every allocation of `call` refused in turn: each refusal is the memory
/// error, the first call it does not reach gives `whole`. Returns how many
/// allocation sites there were.
fn sweep<T: PartialEq + std::fmt::Debug>(whole: &T, call: impl Fn() -> Result<T, Error>) -> usize {
    for (refusals, number) in (1..).enumerate() {
        let (result, refused) = with_refusal(number, &call);
        if !refused {
            assert_eq!(&result.unwrap(), whole);
            return refusals;
        }
        assert_eq!(result, Err(Error::OutOfMemory), "refusal {number}");
    }
    unreachable!()
}

#[test]
fn the_catalog_reads_hold_nothing_and_refuse_every_allocation() {
    let storage = files();
    // The removed book is not counted.
    assert_eq!(catalog::try_count(&storage), Ok(2));
    assert!(sweep(&2, || catalog::try_count(&storage)) >= 3);
    let second = (HEADER.len() + CATALOG.lines().next().unwrap().len() + 1) as u64;
    let first = HEADER.len() as u64;
    let a = Some("books/a.epub".to_owned());
    assert_eq!(catalog::try_path_at(&storage, first), Ok(a.clone()));
    assert_eq!(catalog::try_path_at(&storage, second), Ok(None));
    assert_eq!(catalog::try_path_at(&storage, 100_000), Ok(None));
    assert!(sweep(&a, || catalog::try_path_at(&storage, first)) >= 3);
    let changes = catalog::try_changes(&storage).unwrap();
    assert!(changes.removed(paperpaw_digest::fnv1a64("books/b.epub")));
    assert!(sweep(&false, || Ok(catalog::try_changes(&storage)?.is_empty())) >= 1);
}

#[test]
fn a_status_is_read_whole_or_refused_for_every_allocation() {
    let storage = files();
    let status = library::try_status(&storage, BOOK).unwrap();
    assert_eq!(
        (status.progress, status.favourite, status.opened),
        (Some(40), true, true)
    );
    assert!(sweep(&status, || library::try_status(&storage, BOOK)) >= 3);
    // A book that is not there is the storage's error, not a status.
    assert_eq!(
        library::try_status(&storage, "books/none.epub"),
        Err(Error::Storage(StorageError::NotFound))
    );
}

/// The plain `book` has always stopped the program where there is no
/// memory: a caller takes `None` for a book the library does not know and
/// may write or extract it again, so a shortage is never `None`. The
/// refusal runs in a child of this test binary, which must die by the
/// allocation failure handler.
#[test]
fn the_plain_book_never_answers_none_for_a_memory_shortage() {
    if std::env::var_os("LIBRARY_REFUSED_CHILD").is_some() {
        let storage = files();
        let (book, _) = with_refusal(1, || library::book(&storage, BOOK));
        // Not reached: the handler ends the process.
        std::process::exit(if book.is_none() { 3 } else { 4 });
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_plain_book_never_answers_none_for_a_memory_shortage",
            "--test-threads=1",
        ])
        .env("LIBRARY_REFUSED_CHILD", "1")
        .status()
        .unwrap();
    assert!(!status.success());
    // Neither "answered None" (3) nor "answered a book" (4): it was ended.
    assert_ne!(status.code(), Some(3));
    assert_ne!(status.code(), Some(4));
}

/// A catalog with a book that has everything: a long title, tags, fields.
fn rich() -> (Files, u64, u64) {
    let long = "é".repeat(300); // 600 bytes, cut at 512
    let first = "books/a.epub\tAda\tTitle A\t\t\t\t40\tf\t20000\t\t5\n";
    let second =
        format!("books/d.epub\tDee\t{long}\tSeries\t3\tx\u{1f}y\u{1f}z\t\t\t\tk=v\u{1f}m=n\t9\n");
    let log = format!(
        "{:016x}\t100\tfo\n",
        paperpaw_digest::fnv1a64("books/a.epub")
    );
    let files = Files(vec![
        (
            ".paperpaw/library/catalog.txt".to_owned(),
            format!("{HEADER}{first}{second}").into_bytes(),
        ),
        (".paperpaw/library/status.log".to_owned(), log.into_bytes()),
    ]);
    (
        files,
        HEADER.len() as u64,
        (HEADER.len() + first.len()) as u64,
    )
}

#[test]
fn a_catalog_line_is_seen_cut_and_counted_without_holding_it() {
    let (storage, first, second) = rich();
    let view = |offset| {
        let mut seen = None;
        let found = catalog::try_view_at(&storage, offset, &mut |view| {
            seen = Some((
                view.path.to_owned(),
                view.title.len(),
                view.title_bytes,
                view.series.to_owned(),
                (view.tags, view.fields),
                (
                    view.status.progress,
                    view.status.favourite,
                    view.status.opened,
                ),
                (view.added, view.size),
            ));
        })?;
        Ok::<_, Error>((found, seen))
    };
    // The log's note wins over the catalog's columns.
    let (found, seen) = view(first).unwrap();
    assert!(found);
    let seen = seen.unwrap();
    assert_eq!(seen.0, "books/a.epub");
    assert_eq!(
        (seen.5, seen.6),
        ((Some(100), true, true), (Some(20000), Some(5)))
    );
    // The long title: 512 bytes kept (a whole number of é), 600 whole.
    let (_, seen) = view(second).unwrap();
    let seen = seen.unwrap();
    assert_eq!((seen.1, seen.2, seen.3.as_str()), (512, 600, "Series"));
    assert_eq!((seen.4, seen.6), ((3, 2), (None, Some(9))));
    // No line, a header and a line from nowhere are not books.
    assert!(!view(0).unwrap().0);
    assert!(!view(1_000_000).unwrap().0);
    // Where lines start, by path.
    assert_eq!(
        catalog::try_offset_of(&storage, "books/d.epub"),
        Ok(Some(second))
    );
    assert_eq!(
        catalog::try_offset_of(&storage, "books/none.epub"),
        Ok(None)
    );
    // Every allocation refused: the memory error, nothing half seen.
    let sites = sweep(&second, || {
        catalog::try_offset_of(&storage, "books/d.epub").map(|o| o.unwrap())
    });
    assert!(sites >= 2);
    let sites = sweep(&true, || catalog::try_view_at(&storage, first, &mut |_| {}));
    assert!(sites >= 2);
}

#[test]
fn state_and_generation_are_the_plain_ones() {
    let (storage, ..) = rich();
    assert_eq!(
        catalog::try_state(&storage).unwrap(),
        catalog::state(&storage)
    );
    assert_eq!(
        catalog::try_generation(&storage).unwrap(),
        catalog::generation(&storage)
    );
    assert_eq!(catalog::try_generation(&storage), Ok(Some(7)));
    let none = Files(vec![]);
    assert_eq!(catalog::try_state(&none).unwrap(), catalog::state(&none));
    assert_eq!(catalog::try_generation(&none), Ok(None));
    assert!(sweep(&Some(7), || catalog::try_generation(&storage)) >= 1);
    assert!(sweep(&catalog::state(&storage), || catalog::try_state(&storage)) >= 2);
}
