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
    /// `stars.rating` is kept with the book only, `stars.tag` in the catalog.
    fn book_field(&self, id: &str) -> Option<bool> {
        match id {
            "stars.rating" => Some(false),
            "stars.tag" => Some(true),
            _ => None,
        }
    }
    /// Plugins keep data for book E only.
    fn book_data_kept(&self, path: &str) -> Result<bool, i32> {
        Ok(path == "books/e.epub")
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
            .with("books/a.epub", vec![7_u8; 3000])
            .with("books/c.epub", vec![7_u8; 10])
            .with("books/d.txt", vec![7_u8; 10])
            .with("books/e.epub", vec![7_u8; 10]),
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
    let mut got = got.unwrap();
    // As the service does for op 3: bit 8 `missing` from the file lookup.
    if !reads.book_file_present(&got.path)? {
        got.flags |= 1 << 8;
    }
    Ok(got)
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
    // Cut title; its file is not on the card (bit 8).
    assert_eq!(b.flags, 1 << 5 | 1 << 8);
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
    // A book without annotations has none by id; a kind past 6 is no text.
    assert_eq!(text(&reads, a, 5, 1, 0).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(text(&reads, a, 6, 1, 0).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(text(&reads, a, 7, 0, 0).unwrap_err(), ERR_UNSUPPORTED);
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
    fn query(
        &self,
        query: &wire::ServiceLibQuery<'_>,
        archive: &str,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.0.query(query, archive, visit)
    }
    fn state(&self, visit: One<'_>) -> Result<(), i32> {
        self.0.state(visit)
    }
    fn book(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.0.book(book, visit)
    }
    fn book_file_present(&self, path: &str) -> Result<bool, i32> {
        self.0.book_file_present(path)
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
    fn position(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.0.position(book, visit)
    }
    fn annotations(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: crate::native_host::lib_service::Rows<'_>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        self.0.annotations(book, cursor, generation, visit)
    }
    fn anchors(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: crate::native_host::lib_service::Rows<'_>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        self.0.anchors(book, cursor, generation, visit)
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

fn position(reads: &CatalogReads, book: BookRef<'_>) -> Result<[u64; 7], i32> {
    let mut got = None;
    reads.position(book, &mut |record| {
        let Record::ServiceLibPosition(p) = record else {
            panic!("a position")
        };
        got = Some([
            p.section.into(),
            p.offset.into(),
            p.progress_ppm.into(),
            p.conversion,
            p.updated,
            p.xpointer.len() as u64,
            p.flags.into(),
        ]);
    })?;
    Ok(got.unwrap())
}

/// A card that answers `error` for one path (a failing card, a card that is
/// not mounted, a path it cannot name), for the file and the directory alike.
struct Failing(
    Rc<MemoryStorage>,
    &'static str,
    paperpaw_platform::StorageError,
);
impl paperpaw_platform::Storage for Failing {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        if path == self.1 {
            Err(self.2)
        } else {
            self.0.size(path)
        }
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        if dir == self.1 {
            Err(self.2)
        } else {
            self.0.list(dir, visit)
        }
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(path)
    }
    // As FAT and the desktop's folders: a directory has no size, so by
    // default it does not "exist" either.
}

#[test]
fn a_book_whose_file_is_gone_says_missing() {
    const MISSING: u32 = 1 << 8;
    let card = card();
    let reads = card.reads();
    let flags = |path| book(&reads, BookRef::Path(path)).unwrap().flags;
    // The file is there: no bit; the other flags are as they were.
    assert_eq!(flags("books/a.epub"), 0b11);
    // Gone from the card: bit 8, and only that changes.
    card.storage.remove("books/a.epub").unwrap();
    assert_eq!(flags("books/a.epub"), 0b11 | MISSING);
    card.storage.remove("books/c.epub").unwrap();
    assert_eq!(flags("books/c.epub"), 0b110 | MISSING);
    // Back again: clear. The catalog line is untouched throughout.
    card.storage.write("books/a.epub", &[7_u8; 10]).unwrap();
    assert_eq!(flags("books/a.epub"), 0b11);
    // A card that fails for the file, or is not mounted, is the storage's
    // error, never "gone" (a catalog on another storage still answers).
    use paperpaw_platform::StorageError;
    for error in [StorageError::Io, StorageError::CARD_GONE] {
        let failing = Failing(card.storage.clone(), "books/e.epub", error);
        let reads = CatalogReads::new(Rc::new(failing), card.env.clone());
        assert_eq!(
            book(&reads, BookRef::Path("books/e.epub")).unwrap_err(),
            ERR_IO,
            "{error:?}"
        );
        assert!(book(&reads, BookRef::Path("books/b.epub")).is_ok());
    }
    // A root probe refused memory is a memory error, not a storage one.
    let short = Failing(card.storage.clone(), "", StorageError::OutOfMemory);
    let reads = CatalogReads::new(Rc::new(short), card.env.clone());
    card.storage.remove("books/b.epub").unwrap();
    assert_eq!(
        book(&reads, BookRef::Path("books/b.epub")).unwrap_err(),
        ERR_NO_MEMORY
    );
    card.storage.write("books/b.epub", &[7_u8; 10]).unwrap();
    // A path the storage cannot name is a book that is not there, not a
    // failure of the whole call.
    let odd = Failing(
        card.storage.clone(),
        "books/e.epub",
        StorageError::InvalidPath,
    );
    let reads = CatalogReads::new(Rc::new(odd), card.env.clone());
    assert_eq!(
        book(&reads, BookRef::Path("books/e.epub")).unwrap().flags,
        MISSING | 0b1000,
        "gone, and still new"
    );
}

#[test]
fn a_folder_book_is_there_while_its_directory_is() {
    const MISSING: u32 = 1 << 8;
    let card = card();
    // A book that is a folder (a comic of pictures): no size of its own.
    card.storage
        .append(
            ".paperpaw/library/catalog.txt",
            "books/comic\tCo\tComic\t\t\t\t\t\t\t\t1\n".as_bytes(),
        )
        .unwrap();
    card.storage
        .write("books/comic/p1.png", &[1_u8; 10])
        .unwrap();
    card.storage
        .write("books/comic/p2.png", &[1_u8; 10])
        .unwrap();
    let reads = card.reads();
    let comic = BookRef::Path("books/comic");
    // The folder has no size and no `exists`, yet it is there.
    assert!(card.storage.size("books/comic").is_err());
    assert!(!card.storage.exists("books/comic"));
    assert_eq!(book(&reads, comic).unwrap().flags & MISSING, 0);
    // `MemoryStorage` has no empty folders: with its last picture removed the
    // folder is gone (on FAT an emptied folder still exists, and is there).
    card.storage.remove("books/comic/p1.png").unwrap();
    assert_eq!(book(&reads, comic).unwrap().flags & MISSING, 0);
    card.storage.remove("books/comic/p2.png").unwrap();
    assert_eq!(book(&reads, comic).unwrap().flags & MISSING, MISSING);
}

/// A storage that tells a directory's `modified` without a walk, as FAT does,
/// and counts the `list_from` it is asked for.
struct Stats(Rc<MemoryStorage>, std::cell::Cell<u32>);
impl paperpaw_platform::Storage for Stats {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(dir, visit)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, paperpaw_platform::StorageError> {
        self.1.set(self.1.get() + 1);
        self.0.list_from(dir, from, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(path)
    }
    fn modified(&self, path: &str) -> Option<i64> {
        (path == "books/comic").then_some(1_760_000_000)
    }
}

/// A storage that notes each `read_at` it is asked for.
struct Counting(Rc<MemoryStorage>, std::cell::RefCell<Vec<(String, u64)>>);
impl paperpaw_platform::Storage for Counting {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.1.borrow_mut().push((path.to_string(), offset));
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(dir, visit)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, paperpaw_platform::StorageError> {
        self.0.list_from(dir, from, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(path)
    }
}

#[test]
fn a_book_and_its_text_read_the_log_once_and_the_line_in_one_scan() {
    const LOG: &str = ".paperpaw/library/status.log";
    const CATALOG: &str = ".paperpaw/library/catalog.txt";
    let card = card();
    let counting = Rc::new(Counting(card.storage.clone(), Default::default()));
    let reads = CatalogReads::new(counting.clone(), card.env.clone());
    let log_reads = || counting.1.borrow().iter().filter(|(p, _)| p == LOG).count();
    let line_scans = |offset: u64| {
        counting
            .1
            .borrow()
            .iter()
            .filter(|(p, at)| p == CATALOG && *at == offset)
            .count()
    };
    // What one read of the log costs (the log is read in two passes).
    catalog::try_changes(&*counting).unwrap();
    let once = log_reads();
    assert!(once > 0, "the card's log is read");
    assert_ne!(
        card.b % 512,
        0,
        "the heads' chunks do not start at the line"
    );
    counting.1.borrow_mut().clear();
    // Op 3: the log once, and the line (title, author, series, numbers) in
    // one scan from its start.
    book(&reads, BookRef::Path("books/b.epub")).unwrap();
    assert_eq!(log_reads(), once, "op 3 reads the log once");
    assert_eq!(line_scans(card.b), 1, "op 3 scans the line once");
    // Op 4: the same for a text.
    counting.1.borrow_mut().clear();
    let at = wire::ServiceLibTextAt {
        kind: 0,
        index: 0,
        offset: 0,
    };
    reads
        .text(BookRef::Path("books/b.epub"), &at, &mut |_| {})
        .unwrap();
    assert_eq!(log_reads(), once, "op 4 reads the log once");
    assert_eq!(line_scans(card.b), 1);
}

/// The catalog on one storage, the books on another that is `NoStorage`:
/// everything under `.paperpaw` answers, the rest is empty.
struct Split(Rc<MemoryStorage>);
impl Split {
    fn ours(path: &str) -> bool {
        path.starts_with(".paperpaw")
    }
}
impl paperpaw_platform::Storage for Split {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        if Self::ours(path) {
            self.0.read_at(path, offset, buf)
        } else {
            paperpaw_platform::NoStorage.read_at(path, offset, buf)
        }
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        if Self::ours(path) {
            self.0.size(path)
        } else {
            paperpaw_platform::NoStorage.size(path)
        }
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        if Self::ours(dir) {
            self.0.list(dir, visit)
        } else {
            paperpaw_platform::NoStorage.list(dir, visit)
        }
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(path)
    }
}

#[test]
fn a_directory_that_tells_its_modified_is_not_walked() {
    const MISSING: u32 = 1 << 8;
    let card = card();
    card.storage
        .append(
            ".paperpaw/library/catalog.txt",
            "books/comic\tCo\tComic\t\t\t\t\t\t\t\t1\n".as_bytes(),
        )
        .unwrap();
    card.storage
        .write("books/comic/p1.png", &[1_u8; 10])
        .unwrap();
    let stats = Rc::new(Stats(card.storage.clone(), std::cell::Cell::new(0)));
    let reads = CatalogReads::new(stats.clone(), card.env.clone());
    let flags = book(&reads, BookRef::Path("books/comic")).unwrap().flags;
    assert_eq!(flags & MISSING, 0);
    assert_eq!(stats.1.get(), 0, "no walk where `modified` tells");
}

#[test]
fn books_on_a_storage_that_is_nothing_are_an_error_not_removed() {
    let card = card();
    let split = Split(card.storage.clone());
    let reads = CatalogReads::new(Rc::new(split), card.env.clone());
    // The catalog still answers; the books' storage has nothing at all.
    assert_eq!(
        book(&reads, BookRef::Path("books/a.epub")).unwrap_err(),
        ERR_IO
    );
}

#[test]
fn a_position_is_read_with_its_flags_or_is_not_found() {
    use paperpaw_sdk::document::Locator;
    use paperpaw_sdk::positions::{self, Position};
    let card = card();
    let reads = card.reads();
    let a = BookRef::Path("books/a.epub");
    assert_eq!(position(&reads, a), Err(ERR_NOT_FOUND));
    let kept = Position::new(Locator::new(7, 1234), 41.5)
        .conversion(255)
        .xpointer(Some("/body/p[3]".to_string()))
        .updated(1_760_000_000);
    positions::save(&*card.storage, "books/a.epub", &kept).unwrap();
    assert_eq!(
        position(&reads, a),
        Ok([7, 1234, 415_000, 255, 1_760_000_000, 10, 0b11])
    );
    // By handle, and without conversion, XPointer or clock.
    let by_key = BookRef::Key {
        key: u32::try_from(card.a).unwrap(),
        generation: card.generation(),
    };
    positions::save(
        &*card.storage,
        "books/a.epub",
        &Position::new(Locator::new(0, 0), 100.0),
    )
    .unwrap();
    assert_eq!(position(&reads, by_key), Ok([0, 0, 1_000_000, 0, 0, 0, 0]));
    // A place of a book that is not in the catalog is not found.
    positions::save(&*card.storage, "books/zz.epub", &kept).unwrap();
    assert_eq!(
        position(&reads, BookRef::Path("books/zz.epub")),
        Err(ERR_NOT_FOUND)
    );
    // A file that is no position is an error of the card, not "none".
    card.storage
        .write(
            &positions::file("books/a.epub"),
            b"[position]\nsection = 1\n",
        )
        .unwrap();
    assert_eq!(position(&reads, a), Err(ERR_IO));
    // An XPointer the wire cannot carry is treated as absent, never cut: the
    // rest of the place answers, without `has_xpointer`.
    let long = Position::new(Locator::new(1, 2), 1.0)
        .conversion(255)
        .xpointer(Some("x".repeat(1025)));
    positions::save(&*card.storage, "books/a.epub", &long).unwrap();
    assert_eq!(position(&reads, a), Ok([1, 2, 10_000, 255, 0, 0, 0b10]));
    let longest = Position::new(Locator::new(1, 2), 1.0).xpointer(Some("x".repeat(1024)));
    positions::save(&*card.storage, "books/a.epub", &longest).unwrap();
    assert_eq!(position(&reads, a), Ok([1, 2, 10_000, 0, 0, 1024, 0b01]));
}

/// An annotation file of version 2: four annotations and a line that is none.
pub(crate) fn annotation_file(note: &str) -> std::string::String {
    format!(
        "# format 2\n\
         b\t1\t0\t1\t0\t10\t100\t200\t00000000000000ff\t/a\t\tfirst\tshort\n\
         garbage\n\
         h\t2\t5\t2\t9\t20\t0\t0\t\t\t\tsecond\t{note}\n\
         x\t3\t0\t3\t0\t30\t1\t2\t?\t/p\t/q\tfuture\t\n\
         h\t5\t0\t5\t0\t50\t0\t0\t\t\t\tlast\t\n"
    )
}

pub(crate) fn put_annotations(card: &Card, text: &str) {
    card.storage
        .write(
            &paperpaw_sdk::annotations::file("books/a.epub"),
            text.as_bytes(),
        )
        .unwrap();
}

type Page = (Vec<[u64; 5]>, crate::abi7::context::RowsEnd);

/// Annotations or anchors from `cursor`, taking at most `take` rows.
fn listing(
    reads: &CatalogReads,
    anchors: bool,
    cursor: u32,
    generation: u32,
    take: usize,
) -> Result<Page, i32> {
    let mut rows = Vec::new();
    let mut visit = |record: Record<'_>| {
        if rows.len() == take {
            return core::ops::ControlFlow::Break(());
        }
        rows.push(match record {
            Record::ServiceLibAnnotation(a) => [
                a.id.into(),
                a.kind.into(),
                a.flags.into(),
                a.note.len() as u64,
                a.conversion,
            ],
            Record::ServiceLibAnchor(a) => [
                a.id.into(),
                a.flags.into(),
                a.conversion,
                a.start.len() as u64,
                a.end.len() as u64,
            ],
            _ => panic!("a row"),
        });
        core::ops::ControlFlow::Continue(())
    };
    let book = BookRef::Path("books/a.epub");
    let end = if anchors {
        reads.anchors(book, cursor, generation, &mut visit)?
    } else {
        reads.annotations(book, cursor, generation, &mut visit)?
    };
    Ok((rows, end))
}

#[test]
fn annotations_page_by_cursor_under_the_files_generation() {
    let card = card();
    let reads = card.reads();
    put_annotations(&card, &annotation_file("n"));
    let (rows, end) = listing(&reads, false, 0, 0, 10).unwrap();
    // Version 2: the generation 0 is 1 on the wire; ids by place; the line that
    // is no annotation takes none; "x" is another kind (2 + its letter).
    assert_eq!((end.total, end.generation, end.next), (4, 1, 4));
    assert_eq!(
        rows.iter().map(|r| (r[0], r[1])).collect::<Vec<_>>(),
        [(1, 0), (2, 1), (3, 2 + u64::from(b'x')), (4, 1)]
    );
    // noted | has conversion | has XPointers.
    assert_eq!(rows[0][2], 0b11001);
    assert_eq!((rows[0][3], rows[0][4]), (5, 255));
    assert_eq!(rows[1][2], 0b001);
    assert_eq!(rows[2][2], 0b10000, "the conversion is not known");
    // A page of two, then from the cursor.
    let (rows, end) = listing(&reads, false, 0, 0, 2).unwrap();
    assert_eq!((rows.len(), end.total, end.next), (2, 4, 2));
    let (rows, end) = listing(&reads, false, 2, end.generation, 10).unwrap();
    assert_eq!((rows[0][0], rows.len(), end.next), (3, 2, 4));
    // Past the end: nothing, at the end. Another generation is stale.
    let (rows, end) = listing(&reads, false, 9, 1, 10).unwrap();
    assert_eq!((rows.len(), end.next, end.total), (0, 4, 4));
    assert_eq!(listing(&reads, false, 0, 2, 10).unwrap_err(), ERR_STATE);
    // No file: no rows.
    card.storage
        .remove(&paperpaw_sdk::annotations::file("books/a.epub"))
        .unwrap();
    let (rows, end) = listing(&reads, false, 0, 0, 10).unwrap();
    assert_eq!((rows.len(), end.total, end.generation), (0, 0, 1));
}

#[test]
fn a_file_of_version_3_answers_its_ids_and_generation() {
    let card = card();
    let reads = card.reads();
    put_annotations(
        &card,
        "# format 3\n# next-id 20\n# generation 7\n\
         h\t1\t0\t1\t4\t10\t1\t2\t\t\t\ttext\tnote\t17\tkey-a\n\
         b\t2\t0\t2\t0\t20\t0\t0\t\t\t\t\t\t4\t\n",
    );
    let (rows, end) = listing(&reads, false, 0, 0, 10).unwrap();
    assert_eq!((end.generation, end.total), (7, 2));
    assert_eq!(rows.iter().map(|r| r[0]).collect::<Vec<_>>(), [17, 4]);
    // A page of version 3 is stale once the file moves on.
    assert_eq!(listing(&reads, false, 1, 8, 10).unwrap_err(), ERR_STATE);
    assert_eq!(listing(&reads, false, 1, 7, 10).unwrap().1.next, 2);
}

#[test]
fn anchors_are_the_annotations_that_have_a_conversion_or_xpointers() {
    let card = card();
    let reads = card.reads();
    put_annotations(&card, &annotation_file("n"));
    let (rows, end) = listing(&reads, true, 0, 0, 10).unwrap();
    assert_eq!((end.total, end.generation, end.next), (2, 1, 2));
    // [id, flags, conversion, start len, end len]
    assert_eq!(rows, [[1, 0b11, 255, 2, 0], [3, 0b10, 0, 2, 2]]);
    let (rows, end) = listing(&reads, true, 1, 1, 10).unwrap();
    assert_eq!((rows.len(), rows[0][0], end.next), (1, 3, 2));
}

#[test]
fn an_oversized_xpointer_spares_the_rest_of_its_page() {
    let card = card();
    let reads = card.reads();
    // An XPointer of 1,025 bytes (a file edited by hand: our writers refuse
    // it) cannot be carried and is never cut. Rows: good, oversized with a
    // conversion, oversized without, good again.
    let long = "p".repeat(1025);
    let exact = "q".repeat(1024);
    put_annotations(
        &card,
        &format!(
            "# format 2\n\
             h\t1\t0\t1\t4\t10\t0\t0\t00000000000000ff\t/a\t/b\tgood\t\n\
             h\t2\t0\t1\t4\t10\t0\t0\t00000000000000aa\t{long}\t/b\ttoo big\t\n\
             h\t3\t0\t1\t4\t10\t0\t0\t\t/a\t{long}\tno conversion\t\n\
             h\t4\t0\t1\t4\t10\t0\t0\t\t{exact}\t/b\tcap\t\n"
        ),
    );
    // [id, flags, conversion, start len, end len]: the second keeps only its
    // conversion, the third is no anchor, the fourth (1,024 bytes) is whole.
    let (rows, end) = listing(&reads, true, 0, 0, 10).unwrap();
    assert_eq!(
        rows,
        [
            [1, 0b11, 255, 2, 2],
            [2, 0b01, 170, 0, 0],
            [4, 0b10, 0, 1024, 2]
        ]
    );
    assert_eq!((end.total, end.next), (3, 3));
    // The same page, a row at a time.
    let (rows, end) = listing(&reads, true, 1, 1, 1).unwrap();
    assert_eq!((rows[0][0], end.next, end.total), (2, 2, 3));
    // Annotations say the same: has XPointers for the first and the last.
    let (rows, end) = listing(&reads, false, 0, 0, 10).unwrap();
    assert_eq!(end.total, 4);
    let has = |r: &[u64; 5]| r[2] >> 4 & 1;
    assert_eq!(rows.iter().map(has).collect::<Vec<_>>(), [1, 0, 0, 1]);
}

#[test]
fn a_long_note_is_cut_for_its_row_and_read_whole_by_id() {
    let card = card();
    let reads = card.reads();
    let note = "€".repeat(700);
    put_annotations(&card, &annotation_file(&note));
    let (rows, _) = listing(&reads, false, 0, 0, 10).unwrap();
    // Cut at a character in 512 bytes (170 of three), flag 2 and noted.
    assert_eq!((rows[1][2], rows[1][3]), (0b101, 510));
    let book = BookRef::Path("books/a.epub");
    let (total, first) = text(&reads, book, 6, 2, 0).unwrap();
    assert_eq!((total, first.len()), (2100, 1023));
    let (_, last) = text(&reads, book, 6, 2, 2046).unwrap();
    assert_eq!(last.len(), 54);
    assert_eq!(
        text(&reads, book, 5, 2, 0).unwrap(),
        (6, "second".to_string())
    );
    assert_eq!(
        text(&reads, book, 5, 3, 0).unwrap(),
        (6, "future".to_string())
    );
    // No such id, id 0, a note that is empty.
    assert_eq!(text(&reads, book, 6, 9, 0).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(text(&reads, book, 6, 0, 0).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(text(&reads, book, 6, 3, 0).unwrap(), (0, String::new()));
    // Inside a character, or past the end.
    assert_eq!(text(&reads, book, 6, 2, 1).unwrap_err(), ERR_INVALID);
    assert_eq!(text(&reads, book, 6, 2, 2101).unwrap_err(), ERR_INVALID);
}
