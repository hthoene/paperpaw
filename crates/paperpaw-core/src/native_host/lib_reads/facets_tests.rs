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
use paperpaw_platform::Storage;
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
