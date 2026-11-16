#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 5 `facets` over catalogs in memory: the five lists, their counts and
//! jumps, the filters, and paging by cursor under a generation.
extern crate std;
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_reads::CatalogReads;
use crate::native_host::lib_reads::tests::Env;
use crate::native_host::lib_service::LibQuery;
use crate::native_host::registrations::{self as wire, Record};
use alloc::rc::Rc;
use alloc::string::ToString;
use core::cell::Cell;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;
use std::format;
use std::string::String;
use std::vec::Vec;

const HEADER: &str = "\tcatalog\t2\t1\n";
const CATALOG: &str = ".paperpaw/library/catalog.txt";
/// No jump: a book can carry several tags, and lies in one folder of many.
const NO_JUMP: u32 = u32::MAX;

/// A book of a catalog: path, author, title, series, tags (`,` between them)
/// and its flags (`f` a favourite).
fn book(path: &str, author: &str, title: &str, series: &str, tags: &str, flags: &str) -> String {
    let tags = tags.replace(',', "\u{1f}");
    format!("{path}\t{author}\t{title}\t{series}\t\t{tags}\t\t{flags}\t\t\t5\n")
}

fn reads(lines: &[String]) -> (CatalogReads, Rc<MemoryStorage>) {
    let mut text = String::from(HEADER);
    for line in lines {
        text.push_str(line);
    }
    let storage = Rc::new(MemoryStorage::new().with(CATALOG, text.into_bytes()));
    let env = Rc::new(Env {
        walking: Cell::new(None),
        today: Cell::new(Some(20_000)),
    });
    (CatalogReads::new(storage.clone(), env), storage)
}

fn ask(facet: u32) -> wire::ServiceLibQuery<'static> {
    wire::ServiceLibQuery {
        show: 0,
        sort: 0,
        flags: 0,
        words: "",
        author: "",
        series: "",
        tag: "",
        folder: "",
        added_since: 0,
        cursor: 0,
        generation: 0,
        facet,
    }
}

/// The rows of one call (text, count, first), `room` at most, and its end.
type Rows = Vec<(String, u32, u32)>;

fn page_archived(
    reads: &CatalogReads,
    q: &wire::ServiceLibQuery<'_>,
    archive: &str,
    room: usize,
) -> Result<(Rows, RowsEnd), i32> {
    let mut rows = Vec::new();
    let end = reads.facets(q, archive, &mut |record| {
        if rows.len() == room {
            return ControlFlow::Break(());
        }
        let Record::ServiceLibFacet(row) = record else {
            panic!("a facet row")
        };
        rows.push((row.text.to_string(), row.count, row.first));
        ControlFlow::Continue(())
    })?;
    Ok((rows, end))
}

fn page(reads: &CatalogReads, q: &wire::ServiceLibQuery<'_>) -> (Rows, RowsEnd) {
    page_archived(reads, q, "", 100).unwrap()
}

fn row(text: &str, count: u32, first: u32) -> (String, u32, u32) {
    (text.to_string(), count, first)
}

/// Seven books, as the lists below read them.
fn library() -> Vec<String> {
    std::vec![
        book(
            "books/a.epub",
            "Carroll, Lewis",
            "Alice",
            "Wonderland",
            "classic,children",
            ""
        ),
        book(
            "books/b.epub",
            "kafka",
            "Die Verwandlung",
            "",
            "classic",
            "f"
        ),
        book("books/sf/c.epub", "Herbert", "dune", "Dune", "sf", ""),
        book(
            "books/sf/d.epub",
            "Herbert",
            "Dune Messiah",
            "Dune",
            "sf",
            "f"
        ),
        book("books/sf/deep/e.epub", "", "1984", "", "", ""),
        book(
            "books/crime/f.epub",
            "Carroll, Lewis",
            "Émile",
            "Wonderland",
            "",
            ""
        ),
        book("top.txt", "Austen", "the Hobbit", "", "children", ""),
    ]
}

/// Authors, each whole as the catalog has it and the books without one under
/// the empty name, by their lowercase; `first` is the place of the author's
/// first book in the same books by author.
#[test]
fn the_authors_are_listed_with_their_books_and_where_they_begin() {
    let (reads, _) = reads(&library());
    let (rows, end) = page(&reads, &ask(0));
    assert_eq!(
        rows,
        [
            row("", 1, 0),
            row("Austen", 1, 1),
            row("Carroll, Lewis", 2, 2),
            row("Herbert", 2, 4),
            row("kafka", 1, 6),
        ]
    );
    assert_eq!((end.total, end.next), (5, 5));
    assert_ne!(end.generation, 0);
}

/// Series: a book without one is in none. `first` counts the books by
/// series, where those without one come first.
#[test]
fn the_series_leave_out_the_books_without_one() {
    let (reads, _) = reads(&library());
    let (rows, end) = page(&reads, &ask(1));
    assert_eq!(rows, [row("Dune", 2, 3), row("Wonderland", 2, 5)]);
    assert_eq!((end.total, end.next), (2, 2));
}

/// Tags: a book is under each of its tags, and there is no jump.
#[test]
fn a_book_is_under_each_of_its_tags() {
    let (reads, _) = reads(&library());
    let (rows, end) = page(&reads, &ask(2));
    assert_eq!(
        rows,
        [
            row("children", 2, NO_JUMP),
            row("classic", 2, NO_JUMP),
            row("sf", 2, NO_JUMP),
        ]
    );
    assert_eq!(end.total, 3);
}

/// First letters of the titles, without accents, `#` for a digit; `first` is
/// the place of the letter's first book in the books by title (`émile` is
/// the last of them, `eagle`-like titles come where `e` does).
#[test]
fn the_letters_jump_to_their_first_title() {
    let (reads, _) = reads(&library());
    let (rows, end) = page(&reads, &ask(3));
    // By title: 1984, alice, die verwandlung, dune, dune messiah, the hobbit, émile.
    assert_eq!(
        rows,
        [
            row("#", 1, 0),
            row("A", 1, 1),
            row("D", 3, 2),
            row("E", 1, 6),
            row("T", 1, 5),
        ]
    );
    assert_eq!(end.total, 5);
}

/// Folders, one level: each folder right in the query's folder that holds a
/// book at any depth, by its whole path, with the books in it and below.
#[test]
fn the_folders_are_those_right_inside_the_querys_folder() {
    let (reads, _) = reads(&library());
    let (rows, end) = page(&reads, &ask(4));
    assert_eq!(rows, [row("books", 6, NO_JUMP)]);
    assert_eq!(end.total, 1);
    let inside = wire::ServiceLibQuery {
        folder: "books",
        ..ask(4)
    };
    let (rows, _) = page(&reads, &inside);
    // The two books right in `books` add no row.
    assert_eq!(
        rows,
        [row("books/crime", 1, NO_JUMP), row("books/sf", 3, NO_JUMP)]
    );
}

/// The other fields of the query choose the books that are counted, the
/// archive folder is left out where the query says so, and `first` counts
/// the same books.
#[test]
fn the_querys_other_fields_choose_the_books() {
    let (reads, _) = reads(&library());
    let favourites = wire::ServiceLibQuery { show: 4, ..ask(0) };
    assert_eq!(
        page(&reads, &favourites).0,
        [row("Herbert", 1, 0), row("kafka", 1, 1)]
    );
    let tagged = wire::ServiceLibQuery {
        tag: "classic",
        ..ask(1)
    };
    assert_eq!(page(&reads, &tagged).0, [row("Wonderland", 1, 1)]);
    let without = wire::ServiceLibQuery {
        flags: 0b10,
        ..ask(0)
    };
    let (rows, end) = page_archived(&reads, &without, "books/sf", 100).unwrap();
    assert_eq!(
        rows,
        [
            row("Austen", 1, 0),
            row("Carroll, Lewis", 2, 1),
            row("kafka", 1, 3),
        ]
    );
    assert_eq!(end.total, 3);
    // Not asked to leave it out: the folder counts.
    assert_eq!(
        page_archived(&reads, &ask(0), "books/sf", 100)
            .unwrap()
            .1
            .total,
        5
    );
}

/// A hundred authors in pages: every name once, in order, from the cursor
/// alone; the total is all of them on every page.
#[test]
fn the_names_page_by_cursor_under_one_generation() {
    let lines: Vec<String> = (0..100)
        .map(|n| {
            // Shuffled in the catalog; two books each.
            let author = format!("author {:03}", (n * 37) % 100);
            book(
                &format!("b/{n}.epub"),
                &author,
                &format!("t{n}"),
                "",
                "",
                "",
            ) + &book(
                &format!("b/{n}x.epub"),
                &author,
                &format!("u{n}"),
                "",
                "",
                "",
            )
        })
        .collect();
    let (reads, storage) = reads(&lines);
    let (mut cursor, mut generation, mut seen) = (0, 0, Vec::new());
    loop {
        let q = wire::ServiceLibQuery {
            cursor,
            generation,
            ..ask(0)
        };
        let (rows, end) = page_archived(&reads, &q, "", 7).unwrap();
        assert_eq!(end.total, 100);
        assert_eq!(end.next as usize, seen.len() + rows.len());
        if rows.is_empty() {
            break;
        }
        seen.extend(rows);
        (cursor, generation) = (end.next, end.generation);
    }
    let expected: Rows = (0..100)
        .map(|n| row(&format!("author {n:03}"), 2, n * 2))
        .collect();
    assert_eq!(seen, expected);
    // A cursor past the last name: no row, and the end says where.
    let past = wire::ServiceLibQuery {
        cursor: 500,
        generation,
        ..ask(0)
    };
    let (rows, end) = page(&reads, &past);
    assert!(rows.is_empty());
    assert_eq!((end.total, end.next), (100, 100));
    // The catalog changes: a page of the generation before is stale.
    let mut text = paperpaw_sdk::read_file(&*storage, CATALOG, 1 << 20).unwrap();
    text.extend_from_slice(book("b/new.epub", "zz", "new", "", "", "").as_bytes());
    storage.write(CATALOG, &text).unwrap();
    let stale = wire::ServiceLibQuery {
        cursor: 7,
        generation,
        ..ask(0)
    };
    assert_eq!(page_archived(&reads, &stale, "", 7).unwrap_err(), ERR_STATE);
    assert_eq!(page(&reads, &ask(0)).1.total, 101);
}

/// More names than the count keeps hashes of at once: the total is still
/// every name, and the last page is reached.
#[test]
fn more_names_than_the_count_holds_at_once_are_all_counted() {
    let lines: Vec<String> = (0..3_000)
        .map(|n| book(&format!("b/{n}.epub"), &format!("a{n:04}"), "t", "", "", ""))
        .collect();
    let (reads, _) = reads(&lines);
    let (rows, end) = page_archived(&reads, &ask(0), "", 3).unwrap();
    assert_eq!(end.total, 3_000);
    assert_eq!(
        rows,
        [row("a0000", 1, 0), row("a0001", 1, 1), row("a0002", 1, 2)]
    );
    let last = wire::ServiceLibQuery {
        cursor: 2_998,
        generation: end.generation,
        ..ask(0)
    };
    let (rows, end) = page(&reads, &last);
    assert_eq!(rows, [row("a2998", 1, 2_998), row("a2999", 1, 2_999)]);
    assert_eq!(end.next, 3_000);
}

/// An empty catalog and none at all: no row, no error.
#[test]
fn no_books_are_no_names() {
    let (empty, _) = reads(&[]);
    for facet in 0..=4 {
        let (rows, end) = page(&empty, &ask(facet));
        assert!(rows.is_empty());
        assert_eq!((end.total, end.next), (0, 0));
    }
}

/// A descending query lists the names the other way round, and `first`
/// counts the books in that order.
#[test]
fn a_descending_query_reverses_the_names_and_the_jumps() {
    let (reads, _) = reads(&library());
    let down = wire::ServiceLibQuery {
        flags: 0b1,
        ..ask(0)
    };
    assert_eq!(
        page(&reads, &down).0,
        [
            row("kafka", 1, 0),
            row("Herbert", 2, 1),
            row("Carroll, Lewis", 2, 3),
            row("Austen", 1, 5),
            row("", 1, 6),
        ]
    );
}

/// A book a pending delete holds is in no list, as it is in none of op 2's.
#[test]
fn a_book_being_deleted_is_under_no_name() {
    let (reads, _) = reads(&library());
    let mut rows = Vec::new();
    let end = reads
        .facets_hiding(
            &ask(0),
            "",
            &|path| path.starts_with("books/sf/"),
            &mut |record| {
                let Record::ServiceLibFacet(row) = record else {
                    panic!("a facet row")
                };
                rows.push((row.text.to_string(), row.count, row.first));
                ControlFlow::Continue(())
            },
        )
        .unwrap();
    assert_eq!(
        rows,
        [
            row("Austen", 1, 0),
            row("Carroll, Lewis", 2, 1),
            row("kafka", 1, 3),
        ]
    );
    assert_eq!(end.total, 3);
}

/// Names that differ only in their case are two names, the capital first,
/// and begin together (their books lie mixed, by title); a name longer than
/// a row's text is cut at a character, and a page that is full ends where
/// its last row does.
/// Op 2's key of a series is cut at 85 bytes and a character across the cut
/// is left out: such series begin where the books of their cut begin in op
/// 2's list by series, also where the names' own order (by 96 bytes) puts
/// another series before them.
#[test]
fn a_series_with_a_character_across_the_cut_of_the_key_begins_with_its_cut() {
    let stem = "a".repeat(84);
    let (b, c, f) = (format!("{stem}éb"), format!("{stem}éc"), format!("{stem}f"));
    let (reads, _) = reads(&[
        book("1.epub", "x", "t1", &f, "", ""),
        book("2.epub", "x", "t2", &c, "", ""),
        book("3.epub", "x", "t3", "", "", ""),
        book("4.epub", "x", "t4", &b, "", ""),
        book("5.epub", "x", "t5", &stem, "", ""),
        book("6.epub", "x", "t6", &c, "", ""),
    ]);
    // The list by series: 3 (no series), then 2, 4, 5, 6 (one cut), then 1.
    let mut by_series = ask(0);
    by_series.sort = 2;
    let mut titles = Vec::new();
    reads
        .query_hiding(&by_series, "", &|_| false, &mut |record| {
            if let Record::ServiceLibEntry(row) = record {
                titles.push(row.title.to_string());
            }
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(titles, ["t3", "t2", "t4", "t5", "t6", "t1"]);
    let (rows, end) = page(&reads, &ask(1));
    assert_eq!(
        rows,
        [
            row(&stem, 1, 1),
            row(&f, 1, 5),
            row(&b, 1, 1),
            row(&c, 2, 1)
        ]
    );
    assert_eq!(end.total, 4);
}

#[test]
fn names_keep_their_case_and_long_ones_are_cut() {
    let long = "é".repeat(200);
    let (reads, _) = reads(&[
        book("a.epub", "ann lee", "t", "", "", ""),
        book("b.epub", "Ann Lee", "t", "", "", ""),
        book("c.epub", &long, "t", "", "", ""),
    ]);
    let (rows, end) = page(&reads, &ask(0));
    assert_eq!(rows[..2], [row("Ann Lee", 1, 0), row("ann lee", 1, 0)]);
    // 400 bytes of two-byte characters: 128 of them are 256 bytes.
    assert_eq!(rows[2], row(&"é".repeat(128), 1, 2));
    assert_eq!(end.total, 3);
    let (rows, end) = page_archived(&reads, &ask(0), "", 2).unwrap();
    assert_eq!((rows.len(), end.total, end.next), (2, 3, 2));
}

/// A card that counts the bytes read from the catalog.
struct Counted {
    inner: MemoryStorage,
    read: Cell<u64>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let n = self.inner.read_at(path, at, buf)?;
        if path == CATALOG {
            self.read.set(self.read.get() + n as u64);
        }
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(d, v)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_from(dir, from, visit)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.inner.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.inner.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.inner.sync()
    }
}

/// 1,000 books of 700 authors, 61 series and 29 tags on a card that counts
/// what is read of the catalog, and the catalog's size.
fn counted_shelf() -> (CatalogReads, Rc<Counted>, Rc<Env>, u64) {
    let mut text = String::from(HEADER);
    for n in 0..1_000_u32 {
        text.push_str(&book(
            &format!("books/folder {}/{n:04}.epub", n % 17),
            &format!("Writer {:04}", n % 700),
            &format!("A Usual Title, Number {n}"),
            &format!("Series {}", n % 61),
            &format!("shelf {}", n % 29),
            "",
        ));
    }
    let size = text.len() as u64;
    let storage = Rc::new(Counted {
        inner: MemoryStorage::new().with(CATALOG, text.into_bytes()),
        read: Cell::new(0),
    });
    let env = Rc::new(Env {
        walking: Cell::new(None),
        today: Cell::new(Some(20_000)),
    });
    let reads = CatalogReads::new(storage.clone(), env.clone());
    (reads, storage, env, size)
}

/// How often one call reads the catalog through where it does not go on
/// from the call before: once for each 32 names up to the cursor and once
/// for the page, once more where the rows have a jump, and where a list has
/// more than 512 names, more to count them.
#[test]
fn a_call_reads_the_catalog_as_often_as_the_cursor_is_deep() {
    let (reads, storage, _, size) = counted_shelf();
    let scans = |facet: u32, cursor: u32| {
        storage.read.set(0);
        let mut q = ask(facet);
        q.cursor = cursor;
        let (_, end) = page(&reads, &q);
        assert_ne!(end.total, 0);
        // Whole scans, the changes' log and the header's own reads left out.
        storage.read.get() / size
    };
    let got = [
        scans(0, 0),
        scans(0, 320),
        scans(0, 672),
        scans(1, 0),
        scans(2, 0),
        scans(3, 0),
        scans(4, 0),
    ];
    std::eprintln!(
        "FACETS SCANS of a {size} B catalog (authors from 0, 320, 672; series; tags; letters; \
         folders): {got:?}"
    );
    assert_eq!(got, [4, 14, 25, 2, 1, 2, 1]);
}

/// The rows and the end of a call that hides the books `hide` names, and
/// how often it read the catalog through.
fn counted_page(
    (reads, storage, size): (&CatalogReads, &Counted, u64),
    q: &wire::ServiceLibQuery<'_>,
    hide: &dyn Fn(&str) -> bool,
) -> (Rows, (u32, u32), u64) {
    storage.read.set(0);
    let mut rows = Vec::new();
    let end = reads
        .facets_hiding(q, "", hide, &mut |record| {
            let Record::ServiceLibFacet(row) = record else {
                panic!("a facet row")
            };
            rows.push((row.text.to_string(), row.count, row.first));
            ControlFlow::Continue(())
        })
        .unwrap();
    (rows, (end.total, end.next), storage.read.get() / size)
}

/// A page that goes on where the call before ended reads the catalog twice
/// (its names, their jumps), however deep its cursor is, and answers what a
/// call that starts anew answers: the 700 authors in 46 scans.
#[test]
fn the_page_after_a_page_goes_on_where_that_one_ended() {
    let (reads, storage, _, size) = counted_shelf();
    let none = |_: &str| false;
    let mut q = ask(0);
    let (mut rows, mut end, first) = counted_page((&reads, &storage, size), &q, &none);
    assert_eq!(first, 4);
    let (mut seen, mut scans) = (0, first);
    while !rows.is_empty() {
        seen += rows.len();
        q.cursor = end.1;
        let (anew, anew_storage, _, _) = counted_shelf();
        let expected = counted_page((&anew, &anew_storage, size), &q, &none);
        let more;
        (rows, end, more) = counted_page((&reads, &storage, size), &q, &none);
        assert_eq!((&rows, end), (&expected.0, expected.1), "from {}", q.cursor);
        if rows.is_empty() {
            // Past the last name there are no jumps to count.
            assert_eq!(more, 1);
        } else {
            assert_eq!(more, 2, "from {}", q.cursor);
            scans += more;
        }
    }
    assert_eq!((seen, end, scans), (700, (700, 700), 46));
}

/// The place kept is for its query, its catalog and its day alone: a call
/// of another list, of the other direction, on another day or over a
/// changed catalog starts anew, and so does the next one of the first list.
#[test]
fn a_place_kept_is_for_its_query_its_catalog_and_its_day_alone() {
    let (reads, storage, env, size) = counted_shelf();
    let none = |_: &str| false;
    let on = (&reads, &*storage, size);
    let second = wire::ServiceLibQuery {
        cursor: 32,
        ..ask(0)
    };
    let anew = |q: &wire::ServiceLibQuery<'_>| {
        let fresh = CatalogReads::new(storage.clone(), env.clone());
        let (rows, end, _) = counted_page((&fresh, &storage, size), q, &none);
        (rows, end)
    };
    // Another list in between took the place.
    assert_eq!(counted_page(on, &ask(0), &none).2, 4);
    assert_eq!(counted_page(on, &ask(1), &none).2, 2);
    let (rows, end, scans) = counted_page(on, &second, &none);
    assert_eq!(((rows, end), scans), (anew(&second), 5));
    // The other direction is another query.
    let back = wire::ServiceLibQuery { flags: 1, ..second };
    let (rows, end, scans) = counted_page(on, &back, &none);
    assert_eq!(((rows, end), scans), (anew(&back), 5));
    // Another day.
    assert_eq!(counted_page(on, &ask(0), &none).2, 4);
    env.today.set(Some(20_001));
    assert_eq!(counted_page(on, &second, &none).2, 5);
    // A catalog that changed.
    assert_eq!(counted_page(on, &ask(0), &none).2, 4);
    let mut text = paperpaw_sdk::read_file(&*storage, CATALOG, 1 << 20).unwrap();
    text.extend_from_slice(book("b/new.epub", "A New Writer", "new", "", "", "").as_bytes());
    storage.write(CATALOG, &text).unwrap();
    let grown = text.len() as u64;
    let (rows, end, scans) = counted_page((&reads, &*storage, grown), &second, &none);
    let fresh = CatalogReads::new(storage.clone(), env.clone());
    let expected = counted_page((&fresh, &*storage, grown), &second, &none);
    assert_eq!((rows, end, scans), (expected.0, expected.1, 5));
    assert_eq!((end.0, expected.2), (701, 5));
}

/// While a pending delete hides a book, no place is used and none is kept:
/// a name the hidden book was the last of is gone from before the place, and
/// the page begins one name later, as a call that starts anew has it.
#[test]
fn a_book_being_deleted_uses_no_place_and_keeps_none() {
    let (reads, storage, env, size) = counted_shelf();
    let on = (&reads, &*storage, size);
    let none = |_: &str| false;
    // Both books of the sixth author.
    let hide = |path: &str| path.ends_with("/0005.epub") || path.ends_with("/0705.epub");
    let anew = |q: &wire::ServiceLibQuery<'_>| {
        let fresh = CatalogReads::new(storage.clone(), env.clone());
        let (rows, end, _) = counted_page((&fresh, &storage, size), q, &hide);
        (rows, end)
    };
    assert_eq!(counted_page(on, &ask(0), &none).2, 4);
    let second = wire::ServiceLibQuery {
        cursor: 32,
        ..ask(0)
    };
    // The place is tried (one scan), found hiding a book, and left.
    let (rows, end, scans) = counted_page(on, &second, &hide);
    assert_eq!(rows[0].0, "Writer 0033");
    assert_eq!(((rows, end), scans), (anew(&second), 6));
    // None was kept by that call.
    let third = wire::ServiceLibQuery {
        cursor: 64,
        ..ask(0)
    };
    let (rows, end, scans) = counted_page(on, &third, &hide);
    assert_eq!(((rows, end), scans), (anew(&third), 6));
}

/// The 700 authors read `room` rows a call in the direction of `flags`, each
/// call going on where the one before ended: every page after the first is
/// what a call that starts anew answers, read in two scans. The names in the
/// order they came.
fn walked(flags: u32, room: usize) -> Vec<String> {
    let (reads, storage, env, size) = counted_shelf();
    let take = |reads: &CatalogReads, q: &wire::ServiceLibQuery<'_>| {
        storage.read.set(0);
        let mut rows = Vec::new();
        let end = reads
            .facets_hiding(q, "", &|_| false, &mut |record| {
                if rows.len() == room {
                    return ControlFlow::Break(());
                }
                let Record::ServiceLibFacet(row) = record else {
                    panic!("a facet row")
                };
                rows.push((row.text.to_string(), row.count, row.first));
                ControlFlow::Continue(())
            })
            .unwrap();
        (rows, (end.total, end.next), storage.read.get() / size)
    };
    let mut q = wire::ServiceLibQuery { flags, ..ask(0) };
    let mut names = Vec::new();
    loop {
        let anew = CatalogReads::new(storage.clone(), env.clone());
        let expected = take(&anew, &q);
        let (rows, end, scans) = take(&reads, &q);
        assert_eq!((&rows, end), (&expected.0, expected.1), "from {}", q.cursor);
        if rows.is_empty() {
            return names;
        }
        if q.cursor != 0 {
            assert_eq!(scans, 2, "from {}", q.cursor);
        }
        names.extend(rows.into_iter().map(|row| row.0));
        q.cursor = end.1;
    }
}

/// A caller that takes fewer rows than the page has (20 of 32) goes on
/// after the last row it took, not after the last one the page had.
#[test]
fn a_page_the_caller_cut_short_goes_on_after_its_last_row() {
    let names = walked(0, 20);
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!((names.len(), &names), (700, &sorted));
}

/// A descending list goes on from a place as an ascending one does: whole
/// pages and cut ones are the ascending names the other way round.
#[test]
fn a_descending_page_goes_on_where_the_one_before_ended() {
    let mut up = walked(0, 32);
    assert_eq!(up.len(), 700);
    up.reverse();
    assert_eq!(walked(1, 32), up);
    assert_eq!(walked(1, 20), up);
}
