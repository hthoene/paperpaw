//! A book that could not be indexed for want of memory is not left
//! "indexed" with values it does not have (it is returned not indexed and
//! not written down), and the next walk makes its catalog line anew: every
//! allocation of a walk refused in turn, then an unrefused walk gives the
//! line of a book that was read.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static UNTIL: Cell<Option<usize>> = const { Cell::new(None) };
    static REFUSED: Cell<bool> = const { Cell::new(false) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
    /// The test's own fixtures allocate while this is set: never refused.
    static SHIELD: Cell<bool> = const { Cell::new(false) };
    /// The next allocation of exactly this many bytes is refused (0: none).
    static SIZE: Cell<usize> = const { Cell::new(0) };
}

fn note(size: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

fn refuse() -> bool {
    if SHIELD.try_with(Cell::get).unwrap_or(false) {
        return false;
    }
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
        note(layout.size());
        if SIZE
            .try_with(|size| size.get() == layout.size() && SHIELD.with(|s| !s.get()))
            .unwrap_or(false)
        {
            SIZE.with(|size| size.set(0));
            REFUSED.with(|refused| refused.set(true));
            return core::ptr::null_mut();
        }
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
        note(size);
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
    LARGEST.with(|largest| largest.set(0));
    UNTIL.with(|left| left.set(Some(number)));
    let reset = Reset;
    let result = operation();
    drop(reset);
    (result, REFUSED.with(Cell::get))
}

use std::borrow::Cow;

use paperpaw_sdk::document::{Block, Document, DocumentError, Locator, Metadata};
use paperpaw_sdk::library::{
    self,
    catalog::{self, Walk},
};
use paperpaw_sdk::{DirEntry, Env, FormatSpec, Formats, Id, MemoryStorage, Storage, StorageError};

/// A book that names itself.
struct Named {
    metadata: Metadata,
}

impl Document for Named {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    fn sections(&self) -> u32 {
        1
    }
    fn section_len(&self, _: u32) -> u32 {
        1
    }
    fn block_at(&self, _: &dyn Storage, _: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(None)
    }
}

const BOOK: &str = "books/a.txt";

fn harness() -> common::Harness {
    let mut h = common::Harness::new();
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.txt"),
            "TXT",
            vec![Cow::Borrowed("txt")],
            Box::new(|_, _, _| {
                // The book's own allocations are the fixture's, not the
                // library's: not refused.
                Ok(shielded(|| {
                    let mut metadata = Metadata::default();
                    metadata.title = Some("The Refused Title Of The Walk".to_owned());
                    metadata.authors = vec!["Real Author".to_owned()];
                    metadata.subjects = vec!["one".to_owned(), "two".to_owned()];
                    Box::new(Named { metadata }) as Box<dyn Document>
                }))
            }),
        ))
        .unwrap();
    h.formats = formats;
    h
}

/// The card in memory: what it allocates to keep a file is the fixture's,
/// not the library's, so it is never refused.
struct Card(MemoryStorage);

fn shielded<T>(work: impl FnOnce() -> T) -> T {
    SHIELD.with(|shield| shield.set(true));
    let done = work();
    SHIELD.with(|shield| shield.set(false));
    done
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        shielded(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        shielded(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        shielded(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        shielded(|| self.0.append(path, data))
    }
    fn list(&self, path: &str, each: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        shielded(|| self.0.list(path, each))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        shielded(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        shielded(|| self.0.rename(from, to))
    }
}

fn card() -> Card {
    Card(MemoryStorage::new().with(BOOK, "one"))
}

fn walk(env: &Env<'_>) -> bool {
    let Ok(mut walk) = Walk::start(env, Vec::new()) else {
        return false;
    };
    loop {
        match walk.step(env) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(_) => return false,
        }
    }
}

fn title(storage: &dyn Storage) -> Option<String> {
    let mut found = None;
    catalog::each(storage, &mut |r| {
        if r.path == BOOK {
            found = Some(r.title);
        }
    });
    found
}

#[test]
fn a_book_indexed_with_no_memory_is_returned_not_indexed_and_not_written() {
    for nth in 1_usize.. {
        let h = harness();
        let card = card();
        let env = h.env_on(&card);
        let (book, refused) = with_refusal(nth, || library::index(&env, BOOK));
        if !refused {
            assert_eq!(book.title.as_deref(), Some("The Refused Title Of The Walk"));
            assert!(book.indexed);
            return;
        }
        // Whatever was refused: a book is indexed only with what it names.
        assert!(
            !book.indexed || book.title.as_deref() == Some("The Refused Title Of The Walk"),
            "refusal {nth}: indexed with {:?}",
            book.title
        );
        // And what is kept is never an indexed book without its title.
        if let Some(kept) = library::try_book(&card, BOOK).ok().flatten() {
            assert!(
                !kept.indexed || kept.title.as_deref() == Some("The Refused Title Of The Walk"),
                "refusal {nth}: kept indexed with {:?}",
                kept.title
            );
        }
    }
}

/// What a walk made of the one book when the copy of its title (30 bytes)
/// was refused: the line without the title, and so stale; the next walk,
/// with memory, reads the book again and makes it whole.
#[test]
fn the_next_walk_makes_the_line_of_a_book_left_stale_for_want_of_memory() {
    let h = harness();
    let card = card();
    let env = h.env_on(&card);
    SIZE.with(|size| size.set("The Refused Title Of The Walk".len()));
    REFUSED.with(|refused| refused.set(false));
    assert!(walk(&env));
    SIZE.with(|size| size.set(0));
    assert!(REFUSED.with(Cell::get), "the title's copy was refused");
    // The line names the file for want of the book's title: stale.
    assert_eq!(title(&card).as_deref(), Some("a"), "the line is stale");
    // Not written down as indexed either.
    let kept = library::try_book(&card, BOOK).ok().flatten();
    assert!(kept.is_none_or(|kept| !kept.indexed && kept.title.is_none()));
    assert!(walk(&env));
    assert_eq!(
        title(&card).as_deref(),
        Some("The Refused Title Of The Walk")
    );
}
