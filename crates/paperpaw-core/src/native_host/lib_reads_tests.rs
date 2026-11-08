#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The catalog reads over a catalog on a card in memory: `state`, `book`,
//! `text`, `book_key`, and what they refuse.
extern crate std;
use super::*;
use crate::native_host::lib_service::BookRef;
use alloc::string::ToString;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::digest::{fnv1a64, md5};
use std::cell::Cell;
use std::format;
use std::string::String;
use std::vec;

const HEADER: &str = "\tcatalog\t2\t1\n";

/// The device facts, set by the test.
pub(crate) struct Env {
    pub(crate) walking: Cell<Option<u32>>,
    pub(crate) today: Cell<Option<u32>>,
}
impl CatalogEnv for Env {
    fn walking(&self) -> Option<u32> {
        self.walking.get()
    }
    fn today(&self) -> Option<u32> {
        self.today.get()
    }
    fn format_id(&self, path: &str, visit: &mut dyn FnMut(&str)) {
        if path.ends_with(".epub") {
            visit("epub");
        }
    }
}

/// Five books: A (a favourite, 40 % by the log), B (long title, tags and
/// fields), C (finished), D (removed by the log), E (added 13 days ago, never
/// opened). Today is day 20,003.
pub(crate) struct Card {
    pub(crate) storage: Rc<MemoryStorage>,
    pub(crate) env: Rc<Env>,
    pub(crate) a: u64,
    pub(crate) b: u64,
    pub(crate) c: u64,
    pub(crate) d: u64,
    pub(crate) e: u64,
}

pub(crate) fn long_title() -> String {
    "€".repeat(700)
}

pub(crate) fn card() -> Card {
    let a = "books/a.epub\tAda\tTitle A\t\t\t\t\t\t20000\t\t5\n";
    let b = format!(
        "books/b.epub\tBea\t{}\tSeries\t3\tx\u{1f}y\u{1f}z\t\t\t\tk=v\u{1f}m=n\t9\n",
        long_title()
    );
    let c = "books/c.epub\tCy\tC\t\t\t\t100\to\t10\t\t1\n";
    let d = "books/d.txt\tDee\tD\t\t\t\t\t\t\t\t1\n";
    let e = "books/e.epub\tEve\tE\t\t\t\t\t\t19990\t\t12\n";
    let at = |n: usize| (HEADER.len() + n) as u64;
    let storage = Rc::new(
        MemoryStorage::new()
            .with(
                ".paperpaw/library/catalog.txt",
                format!("{HEADER}{a}{b}{c}{d}{e}").into_bytes(),
            )
            .with("books/a.epub", vec![7_u8; 3000]),
    );
    catalog::note(
        &*storage,
        "books/a.epub",
        catalog::Status::new(Some(40), true, true),
    )
    .unwrap();
    catalog::forget(&*storage, "books/d.txt").unwrap();
    Card {
        storage,
        env: Rc::new(Env {
            walking: Cell::new(None),
            today: Cell::new(Some(20_003)),
        }),
        a: at(0),
        b: at(a.len()),
        c: at(a.len() + b.len()),
        d: at(a.len() + b.len() + c.len()),
        e: at(a.len() + b.len() + c.len() + d.len()),
    }
}

impl Card {
    pub(crate) fn reads(&self) -> CatalogReads {
        CatalogReads::new(self.storage.clone(), self.env.clone())
    }
    /// The generation as `state` says it.
    pub(crate) fn generation(&self) -> u32 {
        state(&self.reads()).2
    }
}

fn state(reads: &CatalogReads) -> (u32, u32, u32, u32) {
    let mut seen = None;
    reads
        .state(&mut |record| {
            let Record::ServiceLibState(s) = record else {
                panic!("a state")
            };
            seen = Some((s.state, s.books, s.generation, s.walk_ppm));
        })
        .unwrap();
    seen.unwrap()
}

/// An owned `lib_book`.
#[derive(Debug, PartialEq)]
struct Got {
    key: u32,
    generation: u32,
    path: String,
    title: String,
    author: String,
    series: String,
    number: String,
    progress: u32,
    flags: u32,
    added: u32,
    size: u64,
    format: String,
    counts: (u32, u32),
    bytes: (u32, u32, u32),
}

fn book(reads: &CatalogReads, book: BookRef<'_>) -> Result<Got, i32> {
    let mut got = None;
    reads.book(book, &mut |record| {
        let Record::ServiceLibBook(b) = record else {
            panic!("a book")
        };
        got = Some(Got {
            key: b.key,
            generation: b.generation,
            path: b.path.to_string(),
            title: b.title.to_string(),
            author: b.author.to_string(),
            series: b.series.to_string(),
            number: b.number.to_string(),
            progress: b.progress,
            flags: b.flags,
            added: b.added,
            size: b.size,
            format: b.format.to_string(),
            counts: (b.tags, b.fields),
            bytes: (b.title_bytes, b.author_bytes, b.series_bytes),
        });
    })?;
    Ok(got.unwrap())
}

fn text(
    reads: &CatalogReads,
    at: BookRef<'_>,
    kind: u32,
    index: u32,
    offset: u32,
) -> Result<(u32, String), i32> {
    let mut got = None;
    let ask = wire::ServiceLibTextAt {
        kind,
        index,
        offset,
    };
    reads.text(at, &ask, &mut |record| {
        let Record::ServiceLibText(t) = record else {
            panic!("a text")
        };
        assert_eq!((t.kind, t.index), (kind, index));
        got = Some((t.total, t.text.to_string()));
    })?;
    Ok(got.unwrap())
}

#[test]
fn the_state_counts_the_books_found_and_says_when_the_walk_is_under_way() {
    let card = card();
    let reads = card.reads();
    // Four books (the log removes the fifth), ready.
    let (s, books, generation, ppm) = state(&reads);
    assert_eq!((s, books, ppm), (2, 4, 1_000_000));
    assert_ne!(generation, 0);
    // A walk that makes the catalog: 1, the books found so far.
    card.env.walking.set(Some(400_000));
    assert_eq!(state(&reads), (1, 4, generation, 400_000));
    // Never 100 % while walking.
    card.env.walking.set(Some(9_000_000));
    assert_eq!(state(&reads).3, 999_999);
    card.env.walking.set(None);
    // A change of status is a change of the catalog.
    catalog::note(
        &*card.storage,
        "books/c.epub",
        catalog::Status::new(Some(50), false, true),
    )
    .unwrap();
    assert_ne!(state(&reads).2, generation);
    // No catalog: none yet, and still a generation.
    let none = CatalogReads::new(Rc::new(MemoryStorage::new()), card.env.clone());
    let (s, books, generation, ppm) = state(&none);
    assert_eq!((s, books, ppm), (0, 0, 0));
    assert_ne!(generation, 0);
}

#[test]
fn a_book_is_read_by_path_or_by_its_handle() {
    let card = card();
    let reads = card.reads();
    let generation = card.generation();
    let by_path = book(&reads, BookRef::Path("books/a.epub")).unwrap();
    assert_eq!(
        by_path,
        Got {
            key: u32::try_from(card.a).unwrap(),
            generation,
            path: "books/a.epub".to_string(),
            title: "Title A".to_string(),
            author: "Ada".to_string(),
            series: String::new(),
            number: String::new(),
            progress: 40,
            // Favourite and opened; not new, it was opened.
            flags: 0b11,
            added: 20_000,
            size: 5,
            format: "epub".to_string(),
            counts: (0, 0),
            bytes: (7, 3, 0),
        }
    );
    let key = BookRef::Key {
        key: u32::try_from(card.a).unwrap(),
        generation,
    };
    assert_eq!(book(&reads, key).unwrap(), by_path);
    // Finished and opened.
    let c = book(&reads, BookRef::Path("books/c.epub")).unwrap();
    assert_eq!((c.progress, c.flags), (100, 0b110));
    // Never opened and added within 30 days: new; not on an unknown clock.
    let e = book(&reads, BookRef::Path("books/e.epub")).unwrap();
    assert_eq!((e.progress, e.flags), (255, 0b1000));
    card.env.today.set(None);
    assert_eq!(
        book(&reads, BookRef::Path("books/e.epub")).unwrap().flags,
        0
    );
    card.env.today.set(Some(19_990 + 31));
    assert_eq!(
        book(&reads, BookRef::Path("books/e.epub")).unwrap().flags,
        0
    );
    card.env.today.set(Some(19_990 + 30));
    assert_eq!(
        book(&reads, BookRef::Path("books/e.epub")).unwrap().flags,
        0b1000
    );
}

#[test]
fn a_long_title_is_cut_at_a_character_and_says_so() {
    let card = card();
    let b = book(&card.reads(), BookRef::Path("books/b.epub")).unwrap();
    // 700 × 3 bytes; 512 holds 170 characters.
    assert_eq!(b.title, "€".repeat(170));
    assert_eq!(b.bytes, (2100, 3, 6));
    assert_eq!(b.flags, 1 << 5);
    assert_eq!((b.series.as_str(), b.number.as_str()), ("Series", "3"));
    assert_eq!(b.counts, (3, 2));
    assert_eq!(b.progress, 255);
}

#[test]
fn an_unknown_removed_stale_or_forged_book_is_refused() {
    let card = card();
    let reads = card.reads();
    let generation = card.generation();
    let key = |key: u64, generation| BookRef::Key {
        key: u32::try_from(key).unwrap(),
        generation,
    };
    assert_eq!(
        book(&reads, BookRef::Path("books/zz.epub")).unwrap_err(),
        ERR_NOT_FOUND
    );
    // Removed by the log, by path and by handle.
    assert_eq!(
        book(&reads, BookRef::Path("books/d.txt")).unwrap_err(),
        ERR_NOT_FOUND
    );
    assert_eq!(
        book(&reads, key(card.d, generation)).unwrap_err(),
        ERR_NOT_FOUND
    );
    // Another generation is stale.
    assert_eq!(
        book(&reads, key(card.a, generation ^ 1)).unwrap_err(),
        ERR_STATE
    );
    assert_eq!(
        text(&reads, key(card.a, generation ^ 1), 0, 0, 0).unwrap_err(),
        ERR_STATE
    );
    // A handle that does not start a line, the header, and one past the end.
    for forged in [0, card.a + 3, card.b - 1, card.e + 1000] {
        assert_eq!(
            book(&reads, key(forged, generation)).unwrap_err(),
            ERR_NOT_FOUND,
            "{forged}"
        );
    }
    assert_eq!(
        text(&reads, key(card.a + 3, generation), 0, 0, 0).unwrap_err(),
        ERR_NOT_FOUND
    );
    // No catalog at all.
    let none = CatalogReads::new(Rc::new(MemoryStorage::new()), card.env.clone());
    assert_eq!(
        book(&none, BookRef::Path("books/a.epub")).unwrap_err(),
        ERR_NOT_FOUND
    );
}

#[test]
fn a_text_is_read_whole_or_in_pieces_from_an_offset() {
    let card = card();
    let reads = card.reads();
    let path = BookRef::Path("books/b.epub");
    assert_eq!(text(&reads, path, 1, 0, 0).unwrap(), (3, "Bea".to_string()));
    assert_eq!(
        text(&reads, path, 2, 0, 0).unwrap(),
        (6, "Series".to_string())
    );
    // The title is 2,100 bytes: pieces of at most 1,024, cut at a character.
    let (total, first) = text(&reads, path, 0, 0, 0).unwrap();
    assert_eq!((total, first.len()), (2100, 1023));
    let (_, second) = text(&reads, path, 0, 0, 1023).unwrap();
    assert_eq!(second.len(), 1023);
    let (_, third) = text(&reads, path, 0, 0, 2046).unwrap();
    assert_eq!(third.len(), 54);
    assert_eq!(format!("{first}{second}{third}"), long_title());
    // At the end: empty; beyond it, or inside a character, the caller's error.
    assert_eq!(
        text(&reads, path, 0, 0, 2100).unwrap(),
        (2100, String::new())
    );
    assert_eq!(text(&reads, path, 0, 0, 2101).unwrap_err(), ERR_INVALID);
    assert_eq!(text(&reads, path, 0, 0, 1).unwrap_err(), ERR_INVALID);
    // Tags and fields by index; one past the list is not found.
    assert_eq!(text(&reads, path, 3, 2, 0).unwrap(), (1, "z".to_string()));
    assert_eq!(text(&reads, path, 4, 1, 0).unwrap(), (3, "m=n".to_string()));
    assert_eq!(text(&reads, path, 3, 3, 0).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(text(&reads, path, 4, 2, 0).unwrap_err(), ERR_NOT_FOUND);
    // A book without tags has none; its title is there.
    let a = BookRef::Path("books/a.epub");
    assert_eq!(text(&reads, a, 3, 0, 0).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(
        text(&reads, a, 0, 0, 0).unwrap(),
        (7, "Title A".to_string())
    );
    // Annotation texts and notes come with the annotation reads.
    assert_eq!(text(&reads, a, 5, 0, 0).unwrap_err(), ERR_UNSUPPORTED);
    assert_eq!(text(&reads, a, 6, 0, 0).unwrap_err(), ERR_UNSUPPORTED);
}

#[test]
fn the_book_key_is_the_hash_of_the_path_with_the_fingerprint() {
    let card = card();
    let reads = card.reads();
    let key = |book| {
        let mut got = None;
        reads
            .book_key(book, &mut |record| {
                let Record::ServiceLibKey(k) = record else {
                    panic!("a key")
                };
                got = Some((k.key.to_string(), k.fingerprint.to_string(), k.size));
            })
            .map(|()| got.unwrap())
    };
    let (hex, fingerprint, size) = key(BookRef::Path("books/a.epub")).unwrap();
    assert_eq!(hex, format!("{:016x}", fnv1a64("books/a.epub")));
    assert_eq!(
        Some(fingerprint.as_str()),
        md5::partial(&*card.storage, "books/a.epub").as_deref()
    );
    assert_eq!(size, 3000);
    let by_handle = BookRef::Key {
        key: u32::try_from(card.a).unwrap(),
        generation: card.generation(),
    };
    assert_eq!(key(by_handle).unwrap(), (hex, fingerprint, size));
    // In the catalog but gone from the card; not a book of the catalog.
    assert_eq!(
        key(BookRef::Path("books/b.epub")).unwrap_err(),
        ERR_NOT_FOUND
    );
    assert_eq!(
        key(BookRef::Path("books/zz.epub")).unwrap_err(),
        ERR_NOT_FOUND
    );
}

#[test]
fn the_catalog_file_is_where_the_sdk_writes_it() {
    let storage = MemoryStorage::new();
    catalog::write(&storage, &[]).unwrap();
    assert!(storage.get(&catalog_file().unwrap()).is_some());
}

/// A whole `LibHost` for the service tests: the catalog reads, no writes.
pub(crate) struct Host(pub(crate) CatalogReads);
impl LibQuery for Host {
    fn state(&self, visit: One<'_>) -> Result<(), i32> {
        self.0.state(visit)
    }
    fn book(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.0.book(book, visit)
    }
    fn text(
        &self,
        book: BookRef<'_>,
        at: &wire::ServiceLibTextAt,
        visit: One<'_>,
    ) -> Result<(), i32> {
        self.0.text(book, at, visit)
    }
    fn book_key(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.0.book_key(book, visit)
    }
}
impl crate::native_host::lib_service::LibWrite for Host {}
impl crate::native_host::lib_service::LibExcluded for Host {
    fn excluded(&self, _: &str) -> bool {
        false
    }
}

#[test]
fn stored_bytes_that_are_not_text_are_an_error_not_an_empty_answer() {
    // A title that starts with a byte that is no UTF-8: 3 bytes, none of
    // them text.
    let mut catalog = HEADER.as_bytes().to_vec();
    catalog.extend_from_slice(b"books/x.epub\tAnn\t\xff\xfe\xfd\t\t\t\t\t\t\t\t1\n");
    let storage = Rc::new(MemoryStorage::new().with(".paperpaw/library/catalog.txt", catalog));
    let card = card();
    let reads = CatalogReads::new(storage, card.env.clone());
    let path = BookRef::Path("books/x.epub");
    assert_eq!(text(&reads, path, 0, 0, 0).unwrap_err(), ERR_INVALID);
    // Nothing is left past the end: the empty answer is right there.
    assert_eq!(text(&reads, path, 0, 0, 3).unwrap(), (3, String::new()));
    // The author is text; the book still reads, its title cut to nothing.
    assert_eq!(text(&reads, path, 1, 0, 0).unwrap(), (3, "Ann".to_string()));
    let b = book(&reads, path).unwrap();
    assert_eq!(
        (b.title.as_str(), b.bytes.0, b.flags & (1 << 5)),
        ("", 3, 1 << 5)
    );
}
