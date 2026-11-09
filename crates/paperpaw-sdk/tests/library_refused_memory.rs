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
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

fn note(size: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
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
        note(layout.size());
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

/// A catalog with a book that has everything: a long title, tags, fields,
/// and one whose number has a 3-byte character across the cap.
fn rich() -> (Files, u64, u64, u64) {
    let long = "é".repeat(300); // 600 bytes
    let first = "books/a.epub\tAda\tTitle A\t\t\t\t40\tf\t20000\t\t5\n";
    let second =
        format!("books/d.epub\tDee\t{long}\tSeries\t3\tx\u{1f}y\u{1f}z\t\t\t\tk=v\u{1f}m=n\t9\n");
    // 33 bytes of "€": cut at the cap of 32 is inside the 11th character.
    let third = format!("books/e.epub\tEve\tT\t\t{}\t\t\t\t\t\t1\n", "€".repeat(11));
    let log = format!(
        "{:016x}\t100\tfo\n",
        paperpaw_digest::fnv1a64("books/a.epub")
    );
    let files = Files(vec![
        (
            ".paperpaw/library/catalog.txt".to_owned(),
            format!("{HEADER}{first}{second}{third}").into_bytes(),
        ),
        (".paperpaw/library/status.log".to_owned(), log.into_bytes()),
    ]);
    let (a, d) = (HEADER.len(), HEADER.len() + first.len());
    (files, a as u64, d as u64, (d + second.len()) as u64)
}

/// What the test keeps of a `catalog::View`, which only lives in the
/// visitor.
#[derive(Debug)]
struct Seen {
    path: String,
    number: String,
    lengths: (u32, u32, u32),
    counts: (u32, u32),
    status: (Option<u8>, bool, bool),
    added: Option<u32>,
    size: Option<u64>,
}

/// What `try_view_at` shows of the line at `offset`.
fn view_of(storage: &Files, offset: u64) -> Result<Option<Seen>, Error> {
    let mut seen = None;
    catalog::try_view_at(storage, offset, &mut |view| {
        seen = Some(Seen {
            path: view.path.to_owned(),
            number: view.number.to_owned(),
            lengths: (view.author_bytes, view.title_bytes, view.series_bytes),
            counts: (view.tags, view.fields),
            status: (
                view.status.progress,
                view.status.favourite,
                view.status.opened,
            ),
            added: view.added,
            size: view.size,
        });
    })?;
    Ok(seen)
}

#[test]
fn a_catalog_line_is_seen_counted_and_cut_without_holding_it() {
    let (storage, first, second, third) = rich();
    // The log's note wins over the catalog's columns.
    let a = view_of(&storage, first).unwrap().unwrap();
    assert_eq!(a.path, "books/a.epub");
    assert_eq!(a.status, (Some(100), true, true));
    assert_eq!((a.added, a.size, a.lengths.1), (Some(20000), Some(5), 7));
    // The long title: its whole length is told, 3 tags, 2 fields.
    let d = view_of(&storage, second).unwrap().unwrap();
    assert_eq!(d.lengths, (3, 600, 6));
    assert_eq!(
        (d.counts, d.size, d.number.as_str()),
        ((3, 2), Some(9), "3")
    );
    // A number is cut at a character, never inside the 3-byte "€".
    let e = view_of(&storage, third).unwrap().unwrap();
    assert_eq!(e.number, "€".repeat(10), "30 of 33 bytes");
    // No line, a header and a line from nowhere are not books.
    assert!(view_of(&storage, 0).unwrap().is_none());
    assert!(view_of(&storage, 1_000_000).unwrap().is_none());
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
fn one_scan_reads_the_numbers_and_the_three_texts_as_the_separate_reads_do() {
    let (storage, first, _second, third) = rich();
    let changes = catalog::try_changes(&storage).unwrap();
    for offset in [first, third] {
        // Buffers of 5 bytes cut the longer texts; the whole lengths stay.
        let (mut title, mut author, mut series) = ([0_u8; 5], [0_u8; 5], [0_u8; 5]);
        let mut got = None;
        let seen = catalog::try_book_at_with(
            &storage,
            &changes,
            offset,
            catalog::BookTexts::new(&mut title, &mut author, &mut series),
            &mut |view, filled| got = Some((view.title_bytes, view.author_bytes, filled)),
        )
        .unwrap();
        assert!(seen);
        let (title_bytes, author_bytes, filled) = got.unwrap();
        for (column, whole, bytes, kept) in [
            (
                catalog::TextColumn::Title,
                title_bytes,
                &title,
                filled.title,
            ),
            (
                catalog::TextColumn::Author,
                author_bytes,
                &author,
                filled.author,
            ),
        ] {
            let mut alone = [0_u8; 5];
            let text =
                catalog::try_text_at_with(&storage, &changes, offset, column, 0, 0, &mut alone)
                    .unwrap()
                    .unwrap();
            assert_eq!((text.total, text.filled), (whole, kept), "{column:?}");
            assert_eq!(&alone[..kept], &bytes[..kept], "{column:?}");
        }
        // The view with the caller's log is the one that reads its own.
        let mut with = None;
        catalog::try_view_at_with(&storage, &changes, offset, &mut |v| {
            with = Some(v.path.to_owned())
        })
        .unwrap();
        assert_eq!(with, view_of(&storage, offset).unwrap().map(|s| s.path));
    }
    // A removed book and a place that is no line are not seen.
    assert_eq!(
        catalog::try_offset_of_with(&storage, &changes, "books/none.epub"),
        Ok(None)
    );
    assert_eq!(
        catalog::try_book_at_with(
            &storage,
            &changes,
            1_000_000,
            catalog::BookTexts::new(&mut [], &mut [], &mut []),
            &mut |_, _| panic!("no book there"),
        ),
        Ok(false)
    );
}

#[test]
fn a_path_that_is_not_text_or_a_line_that_is_too_long_is_no_book() {
    let mut bad = b"\tcatalog\t2\t7\nbooks/\xff.epub\tAda\tT\t\t\t\t\t\t\t\t1\n".to_vec();
    let head = 14;
    let long_at = bad.len() as u64;
    bad.extend_from_slice(b"books/z.epub\tAda\t");
    bad.extend(std::iter::repeat_n(b'x', 20_000));
    bad.push(b'\n');
    let storage = Files(vec![(".paperpaw/library/catalog.txt".to_owned(), bad)]);
    // Neither is cut and passed on as a book.
    assert!(view_of(&storage, head).unwrap().is_none());
    assert!(view_of(&storage, long_at).unwrap().is_none());
    let mut out = [0_u8; 8];
    let mut text = |offset| {
        catalog::try_text_at(
            &storage,
            offset,
            catalog::TextColumn::Author,
            0,
            0,
            &mut out,
        )
    };
    assert_eq!(text(head), Ok(None));
    assert_eq!(text(long_at), Ok(None));
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

#[test]
fn a_long_text_or_a_list_item_is_read_from_a_byte_on_without_holding_the_line() {
    use catalog::TextColumn::{Author, Field, Tag, Title};
    let (storage, first, second, _) = rich();
    let read = |offset, column, part, from| {
        let mut out = [0_u8; 1024];
        let text = catalog::try_text_at(&storage, offset, column, part, from, &mut out)?;
        Ok::<_, Error>(text.map(|t| (t.total, out[..t.filled].to_vec())))
    };
    // The whole title is 600 bytes; from byte 512 the last 88 are left.
    let (total, bytes) = read(second, Title, 0, 0).unwrap().unwrap();
    assert_eq!((total, bytes.len()), (600, 600));
    let (total, tail) = read(second, Title, 0, 512).unwrap().unwrap();
    assert_eq!((total, tail.as_slice()), (600, &bytes[512..]));
    assert_eq!(read(second, Author, 0, 0).unwrap().unwrap().1, b"Dee");
    // Tags and fields are the n-th item; one past the last is no item.
    assert_eq!(read(second, Tag, 1, 0).unwrap().unwrap().1, b"y");
    assert_eq!(read(second, Field, 1, 0).unwrap().unwrap().1, b"m=n");
    assert_eq!(read(second, Tag, 3, 0), Ok(None));
    assert_eq!(read(second, Title, 1, 0), Ok(None));
    // A book without tags has no first tag; a title is there even if empty.
    assert_eq!(read(first, Tag, 0, 0), Ok(None));
    assert_eq!(read(first, Title, 0, 0).unwrap().unwrap().1, b"Title A");
    assert_eq!(read(1_000_000, Title, 0, 0), Ok(None));
    // Nothing is allocated inside the sweep: the answer is a length and a byte.
    let sites = sweep(&Some((1, 1, b'y')), || {
        let mut out = [0_u8; 8];
        let text = catalog::try_text_at(&storage, second, Tag, 1, 0, &mut out)?;
        Ok(text.map(|t| (t.total, t.filled, out[0])))
    });
    assert!(sites >= 2);
}

fn titled(path: &str, title: &str, author: &str) -> catalog::Record {
    let mut record = catalog::Record::default();
    record.path = path.into();
    record.title = title.into();
    record.author = author.into();
    record
}

fn shelf() -> Vec<catalog::Record> {
    vec![
        titled("b/c.epub", "Charlie", "Zed"),
        titled("b/a.epub", "alpha", "Yan"),
        titled("b/d.epub", "Delta", "Xi"),
        titled("b/b.epub", "Bravo", "Yan"),
        titled("b/g.epub", "ΟΔΟΣ Ἀθηνᾶς", "Σοφοκλῆς"),
    ]
}

/// The catalog file of `shelf()` as the library writes it, `extra` lines
/// after the header.
fn catalog_text(extra: &str) -> Vec<u8> {
    let mut text = String::from("\tcatalog\t2\t1\n");
    for record in shelf() {
        text.push_str(&format!(
            "{}\t{}\t{}\t\t\t\t\t\t\t\t1\n",
            record.path, record.author, record.title
        ));
    }
    text.push_str(extra);
    text.into_bytes()
}

fn on_card(text: Vec<u8>) -> Files {
    Files(vec![(".paperpaw/library/catalog.txt".into(), text)])
}

fn paths(records: &[catalog::Record]) -> Vec<&str> {
    records.iter().map(|r| r.path.as_str()).collect()
}

#[test]
fn the_order_of_a_query_is_that_of_str_to_lowercase() {
    // Titles whose keys depend on the final sigma, Turkish dotted capital I,
    // sharp s and combining marks: the page order is the stored one.
    let titles = [
        "ΟΔΥΣΣΕΥΣ",
        "ΟΔΥΣ'",
        "ΟΔΥΣ’Α",
        "ΑΣ\u{301}",
        "ΑΣ\u{301}Α",
        "İstanbul",
        "Istanbul",
        "ISTANBUL",
        "Straße",
        "STRASSE",
        "ẞ",
        "e\u{301}a",
        "éa",
        "Éa",
        "ǅ",
        "Ǆ",
        "ΣΑΣ.",
        "ΣΑΣ",
    ];
    let records: Vec<catalog::Record> = titles
        .iter()
        .enumerate()
        .map(|(at, title)| titled(&format!("b/{at}.epub"), title, "Σοφοκλῆς Σ"))
        .collect();
    for sort in [
        catalog::Sort::Title,
        catalog::Sort::Author,
        catalog::Sort::Series,
        catalog::Sort::Added,
    ] {
        let query = catalog::Query::new().sort(sort);
        let books = |f: &mut dyn FnMut(u64, catalog::Record)| {
            for (at, record) in records.iter().enumerate() {
                f(at as u64, record.clone());
            }
        };
        let mut after = None;
        loop {
            let plain = catalog::query_in(&mut |f| books(f), &query, after.as_ref(), 3);
            let tried = catalog::try_query_in(
                &mut |f| {
                    books(f);
                    Ok(())
                },
                &query,
                after.as_ref(),
                3,
            );
            assert_eq!(tried.unwrap(), plain, "{sort:?}");
            let Some(last) = plain.last().cloned() else {
                break;
            };
            after = Some(last);
        }
    }
}

#[test]
fn a_query_page_is_what_the_plain_query_finds() {
    let records = shelf();
    let books = |f: &mut dyn FnMut(u64, catalog::Record)| {
        for (at, record) in records.iter().enumerate() {
            f(at as u64, record.clone());
        }
    };
    let queries = [
        catalog::Query::new(),
        catalog::Query::new().sort(catalog::Sort::Author),
        catalog::Query::new().sort(catalog::Sort::Series),
        catalog::Query::new().sort(catalog::Sort::Added),
        catalog::Query::new().words("ΟΔΟΣ"),
        catalog::Query::new().words("yan bravo"),
        catalog::Query::new().words("ΣΟΦΟΚΛΗΣ ".to_lowercase().trim()),
        catalog::Query::new().author("Yan"),
    ];
    for query in &queries {
        let mut after = None;
        loop {
            let plain = catalog::query_in(&mut |f| books(f), query, after.as_ref(), 2);
            let tried = catalog::try_query_in(
                &mut |f| {
                    books(f);
                    Ok(())
                },
                query,
                after.as_ref(),
                2,
            );
            assert_eq!(tried.unwrap(), plain, "{query:?}");
            let Some(last) = plain.last().cloned() else {
                break;
            };
            after = Some(last);
        }
    }
    // The same through the card: every page, the card's own records.
    let card = on_card(catalog_text(""));
    let all = catalog::try_query(&card, &catalog::Query::new(), None, 10).unwrap();
    assert_eq!(
        paths(&all),
        ["b/a.epub", "b/b.epub", "b/c.epub", "b/d.epub", "b/g.epub"]
    );
    let words = catalog::Query::new().words("yan");
    assert_eq!(
        catalog::try_query(&card, &words, None, 10).unwrap().len(),
        2
    );
    assert!(
        catalog::try_query(&card, &words, None, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_line_over_the_cap_or_without_an_end_is_skipped_never_held() {
    let long = format!("b/long.epub\tA\t{}\t\t\t\t\t\t\t\t1\n", "t".repeat(9_000));
    let card = on_card(catalog_text(&long));
    let (found, _) = with_refusal(usize::MAX, || {
        catalog::try_query(&card, &catalog::Query::new(), None, 10)
    });
    let found = found.unwrap();
    assert_eq!(found.len(), 5, "the long line is no book, the rest are");
    // A damaged line with no newline at all, megabytes of it: skipped, and
    // no allocation ever grows to hold it.
    let junk = "x".repeat(200_000);
    let card = on_card(catalog_text(&format!("b/z.epub\tA\t{junk}")));
    let (found, _) = with_refusal(usize::MAX, || {
        catalog::try_query(&card, &catalog::Query::new(), None, 10)
    });
    assert_eq!(found.unwrap().len(), 5);
    let largest = LARGEST.with(Cell::get);
    assert!(largest <= 16 * 1024, "the largest allocation was {largest}");
    // A last line without its newline, within the cap, is a book.
    let card = on_card(catalog_text("b/y.epub\tA\tYankee\t\t\t\t\t\t\t\t1"));
    assert_eq!(
        catalog::try_query(&card, &catalog::Query::new(), None, 10)
            .unwrap()
            .len(),
        6
    );
}

#[test]
fn a_query_refused_memory_at_any_allocation_is_an_error_never_an_abort() {
    let card = on_card(catalog_text(""));
    let query = catalog::Query::new()
        .sort(catalog::Sort::Author)
        .words("yan");
    let expected = catalog::try_query(&card, &query, None, 2).unwrap();
    assert_eq!(paths(&expected), ["b/a.epub", "b/b.epub"]);
    let mut number = 1;
    loop {
        let (result, refused) = with_refusal(number, || catalog::try_query(&card, &query, None, 2));
        if !refused {
            assert_eq!(result.unwrap(), expected, "after the last allocation");
            break;
        }
        assert_eq!(result, Err(Error::OutOfMemory), "refusal {number}");
        number += 1;
    }
    // The page, the line, the log, a book and its key: more than a few.
    assert!(number > 6, "{number}");
}

#[test]
fn a_key_and_a_word_match_that_grow_when_lowercased_refuse_never_abort() {
    // `İ` is two bytes and lowercases to three: what was reserved for the
    // text is used up before the separator and the tail are pushed. One
    // book on the card, so that nothing but the library allocates.
    let long = "İ".repeat(16);
    let line = format!(
        "b/i.epub\t{long}\t{long}\t{}\t\t\t\t\t\t\t1\n",
        "İ".repeat(8)
    );
    let card = on_card(line.into_bytes());
    let queries = [
        catalog::Query::new().sort(catalog::Sort::Author),
        catalog::Query::new().sort(catalog::Sort::Series),
        catalog::Query::new().words("i\u{307}i\u{307}"),
        catalog::Query::new().words("zzz"),
    ];
    for query in &queries {
        let whole = catalog::try_query(&card, query, None, 2);
        let mut number = 1;
        loop {
            let (result, refused) =
                with_refusal(number, || catalog::try_query(&card, query, None, 2));
            if !refused {
                assert_eq!(result, whole, "after the last allocation");
                break;
            }
            assert_eq!(
                result,
                Err(Error::OutOfMemory),
                "refusal {number}, {query:?}"
            );
            number += 1;
        }
    }
}

/// A card that cannot be read past its first chunk.
struct Faulty(Files);
impl Storage for Faulty {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if offset >= 512 {
            return Err(StorageError::Io);
        }
        self.0.read_at(path, offset, buf)
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
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

#[test]
fn a_card_that_fails_mid_scan_is_an_error_not_a_short_list() {
    // More than one chunk of books; the second read fails.
    let more = "b/more.epub\tA\tMore\t\t\t\t\t\t\t\t1\n".repeat(30);
    let card = Faulty(on_card(catalog_text(&more)));
    let query = catalog::Query::new();
    assert_eq!(
        catalog::try_query(&card, &query, None, 100),
        Err(Error::Storage(StorageError::Io))
    );
    // A catalog that is not there has no books; any other failure is not that.
    let none = Files(Vec::new());
    assert_eq!(catalog::try_query(&none, &query, None, 10), Ok(Vec::new()));
}

/// Files that are read as [`Files`] and written without allocating: what
/// was written is kept as a hash of the bytes and their number.
struct Writing {
    files: Files,
    wrote: Cell<Option<(u64, usize)>>,
}

impl Storage for Writing {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, _: &str, data: &[u8]) -> Result<(), StorageError> {
        let hash = data.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        self.wrote.set(Some((hash, data.len())));
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn list(&self, path: &str, each: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(path, each)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
}

fn writing() -> Writing {
    Writing {
        files: files(),
        wrote: Cell::new(None),
    }
}

/// The details are written whole or not at all: every allocation of
/// `save` refused in turn is the memory error with nothing written, and
/// the first call that is not refused writes what an unrefused one does.
#[test]
fn details_are_written_whole_or_refused_for_every_allocation() {
    let storage = writing();
    let book = library::try_book(&storage, BOOK).unwrap().unwrap();
    library::save(&storage, BOOK, &book).unwrap();
    let kept = storage.wrote.take().expect("written");
    for number in 1.. {
        let (result, refused) = with_refusal(number, || library::save(&storage, BOOK, &book));
        if !refused {
            assert_eq!(result, Ok(()));
            assert_eq!(storage.wrote.take(), Some(kept));
            assert!(number > 8, "only {number} allocation sites");
            break;
        }
        assert_eq!(result, Err(Error::OutOfMemory), "refusal {number}");
        assert_eq!(storage.wrote.take(), None, "refusal {number}");
    }
}

/// Details over what is read back are refused (as before) before the text
/// is made: nothing is written, and nothing near the limit is reserved.
#[test]
fn details_over_the_limit_are_refused_before_the_text_is_made() {
    let storage = writing();
    let mut book = library::try_book(&storage, BOOK).unwrap().unwrap();
    book.fields = (0..40)
        .map(|i| (format!("test.field{i}"), "x".repeat(1000)))
        .collect();
    let (result, refused) = with_refusal(usize::MAX, || library::save(&storage, BOOK, &book));
    assert!(!refused);
    assert_eq!(result, Err(Error::Storage(StorageError::TooLarge)));
    assert_eq!(storage.wrote.take(), None);
    let largest = LARGEST.with(Cell::get);
    assert!(largest < 16 * 1024, "reserved {largest} bytes");
}

/// What a write of the details of [`BOOK`] ends as, for every allocation of
/// `write` refused in turn: nothing is written (and nothing aborts) while one
/// is refused, and the first call that is not refused writes what an
/// unrefused one does. Returns how many allocation sites there were.
fn written_whole_or_not_at_all(storage: &Writing, write: impl Fn(&Writing)) -> usize {
    write(storage);
    let kept = storage.wrote.take().expect("written");
    for number in 1.. {
        let (_, refused) = with_refusal(number, || write(storage));
        if !refused {
            assert_eq!(storage.wrote.take(), Some(kept));
            return number - 1;
        }
        assert_eq!(storage.wrote.take(), None, "refusal {number}");
    }
    unreachable!()
}

#[test]
fn a_field_is_set_whole_or_not_at_all_for_every_allocation() {
    let storage = writing();
    let sites = written_whole_or_not_at_all(&storage, |storage| {
        library::set_field(storage, BOOK, "myplugin.stars", "4");
    });
    assert!(sites >= 5, "only {sites} allocation sites");
    // Removing a field is a write too.
    let sites = written_whole_or_not_at_all(&storage, |storage| {
        library::set_field(storage, BOOK, "myplugin.rating", "");
    });
    assert!(sites >= 3, "only {sites} allocation sites");
}

#[test]
fn what_a_source_provides_is_kept_whole_or_not_at_all_for_every_allocation() {
    let storage = writing();
    // The provided text is made before each call: the call's own allocations
    // are the ones refused.
    let provided = || {
        library::Provided::default()
            .title("Provided".to_owned())
            .author("Ann".to_owned())
            .tags(vec!["a".to_owned(), "b".to_owned()])
    };
    storage.wrote.set(None);
    library::provide(&storage, BOOK, provided());
    let kept = storage.wrote.take().expect("written");
    for number in 1.. {
        let input = provided();
        let (_, refused) = with_refusal(number, || library::provide(&storage, BOOK, input));
        if !refused {
            assert_eq!(storage.wrote.take(), Some(kept));
            assert!(number > 5, "only {number} allocation sites");
            break;
        }
        assert_eq!(storage.wrote.take(), None, "refusal {number}");
    }
}
