#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 2 `query` over catalogs in memory: the filters, the six orders, paging
//! by cursor under a generation, and what a changing catalog does.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::Env;
use crate::native_host::lib_service::{BookRef, LibQuery};
use alloc::rc::Rc;
use alloc::string::ToString;
use core::cell::Cell;
use paperpaw_platform::Storage;
use paperpaw_sdk::MemoryStorage;
use std::format;
use std::string::String;
use std::vec::Vec;

const HEADER: &str = "\tcatalog\t2\t1\n";

/// One row of an answer, owned.
#[derive(Debug, Clone, PartialEq)]
struct Got {
    key: u32,
    title: String,
    author: String,
    series: String,
    number: String,
    progress: u32,
    flags: u32,
    added: u32,
}

/// A line of a catalog: path, author, title, series, number, progress, flags,
/// added.
fn line(
    path: &str,
    author: &str,
    title: &str,
    series: &str,
    number: &str,
    rest: [&str; 3],
) -> String {
    format!(
        "{path}\t{author}\t{title}\t{series}\t{number}\t\t{}\t{}\t{}\t\t5\n",
        rest[0], rest[1], rest[2]
    )
}

struct Shelf {
    storage: Rc<MemoryStorage>,
    env: Rc<Env>,
    /// Where each line starts, in the order they were given.
    at: Vec<u64>,
}

fn shelf(lines: &[String]) -> Shelf {
    let mut text = String::from(HEADER);
    let mut at = Vec::new();
    for l in lines {
        at.push(text.len() as u64);
        text.push_str(l);
    }
    Shelf {
        storage: Rc::new(
            MemoryStorage::new().with(".paperpaw/library/catalog.txt", text.into_bytes()),
        ),
        env: Rc::new(Env {
            walking: Cell::new(None),
            today: Cell::new(Some(20_000)),
        }),
        at,
    }
}

impl Shelf {
    fn reads(&self) -> CatalogReads {
        CatalogReads::new(self.storage.clone(), self.env.clone())
    }
}

fn ask() -> wire::ServiceLibQuery<'static> {
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
        facet: 0,
    }
}

/// The rows of one call and where it ended; `room` rows at most.
fn page(
    reads: &CatalogReads,
    q: &wire::ServiceLibQuery<'_>,
    room: usize,
) -> Result<(Vec<Got>, RowsEnd), i32> {
    page_archived(reads, q, "", room)
}

fn page_archived(
    reads: &CatalogReads,
    q: &wire::ServiceLibQuery<'_>,
    archive: &str,
    room: usize,
) -> Result<(Vec<Got>, RowsEnd), i32> {
    let mut rows = Vec::new();
    let end = reads.query_hiding(q, archive, &|_| false, &mut |record| {
        if rows.len() == room {
            return ControlFlow::Break(());
        }
        let Record::ServiceLibEntry(e) = record else {
            panic!("an entry")
        };
        rows.push(Got {
            key: e.key,
            title: e.title.to_string(),
            author: e.author.to_string(),
            series: e.series.to_string(),
            number: e.number.to_string(),
            progress: e.progress,
            flags: e.flags,
            added: e.added,
        });
        ControlFlow::Continue(())
    })?;
    Ok((rows, end))
}

fn titles(rows: &[Got]) -> Vec<&str> {
    rows.iter().map(|r| r.title.as_str()).collect()
}

/// Forty-five books `b00`... with titles in a shuffled order.
fn many() -> Shelf {
    let lines: Vec<String> = (0..45)
        .map(|n| {
            let title = format!("Book {:02}", (n * 7) % 45);
            line(
                &format!("books/{n:02}.epub"),
                "Ann",
                &title,
                "",
                "",
                ["", "", "19990"],
            )
        })
        .collect();
    shelf(&lines)
}

#[test]
fn the_books_come_by_title_regardless_of_case_with_their_handles() {
    let s = shelf(&[
        line("books/z.epub", "Zed", "zebra", "", "", ["", "", "1"]),
        line(
            "books/a.epub",
            "Ann",
            "Apple",
            "Fruit",
            "2",
            ["40", "fo", "19990"],
        ),
        line("books/m.epub", "Moe", "Mango", "", "", ["100", "o", "1"]),
    ]);
    let (rows, end) = page(&s.reads(), &ask(), 32).unwrap();
    assert_eq!(titles(&rows), ["Apple", "Mango", "zebra"]);
    assert_eq!((end.total, end.next), (3, 3));
    assert_ne!(end.generation, 0);
    // The handle is where the line starts.
    assert_eq!(
        rows.iter().map(|r| u64::from(r.key)).collect::<Vec<_>>(),
        [s.at[1], s.at[2], s.at[0]]
    );
    // Favourite, opened, 40 %: flags 0b11; finished and opened; never opened.
    assert_eq!((rows[0].progress, rows[0].flags & 0b111), (40, 0b011));
    assert_eq!((rows[1].progress, rows[1].flags & 0b111), (100, 0b110));
    assert_eq!((rows[2].progress, rows[2].flags & 0b111), (255, 0));
    assert_eq!(
        (rows[0].series.as_str(), rows[0].number.as_str()),
        ("Fruit", "2")
    );
}

#[test]
fn a_page_is_read_from_its_cursor_without_gaps_or_repeats() {
    let s = many();
    let reads = s.reads();
    let (all, end) = page(&reads, &ask(), 99).unwrap();
    assert_eq!(all.len(), 32, "a window is 32 rows");
    assert_eq!((end.total, end.next), (45, 32));
    let generation = end.generation;
    // The pages of 32 and of 13, each from the cursor the one before ended at.
    let mut q = ask();
    q.cursor = 32;
    q.generation = generation;
    let (second, end) = page(&reads, &q, 99).unwrap();
    assert_eq!((second.len(), end.total, end.next), (13, 45, 45));
    let mut seen: Vec<String> = all.iter().chain(&second).map(|r| r.title.clone()).collect();
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(seen, sorted, "in order");
    seen.dedup();
    assert_eq!(seen.len(), 45, "none twice");
    // Every cursor gives the rows from there: the skip is exact.
    for cursor in [1, 17, 31, 32, 33, 40, 44] {
        let mut q = ask();
        q.cursor = cursor;
        q.generation = generation;
        let (rows, end) = page(&reads, &q, 99).unwrap();
        let from = cursor as usize;
        let want: Vec<&String> = sorted.iter().skip(from).take(32).collect();
        assert_eq!(
            rows.iter().map(|r| &r.title).collect::<Vec<_>>(),
            want,
            "from {cursor}"
        );
        assert_eq!(end.next as usize, from + want.len());
    }
}

#[test]
fn a_cursor_past_the_last_book_is_an_empty_last_page() {
    let s = many();
    let reads = s.reads();
    let generation = page(&reads, &ask(), 1).unwrap().1.generation;
    for cursor in [45, 46, 300] {
        let mut q = ask();
        q.cursor = cursor;
        q.generation = generation;
        let (rows, end) = page(&reads, &q, 99).unwrap();
        assert!(rows.is_empty());
        assert_eq!((end.total, end.next), (45, 45), "cursor {cursor}");
    }
}

#[test]
fn a_page_that_is_cut_short_ends_where_its_rows_do() {
    let s = many();
    let (rows, end) = page(&s.reads(), &ask(), 10).unwrap();
    assert_eq!((rows.len(), end.total, end.next), (10, 45, 10));
}

#[test]
fn descending_is_the_whole_order_turned_round() {
    let s = many();
    let reads = s.reads();
    let mut every = Vec::new();
    let mut generation = 0;
    for cursor in [0, 32] {
        let mut q = ask();
        q.flags = 1;
        q.cursor = cursor;
        q.generation = generation;
        let (rows, end) = page(&reads, &q, 99).unwrap();
        generation = end.generation;
        every.extend(rows.into_iter().map(|r| r.title));
    }
    assert_eq!(every.len(), 45);
    let mut want = every.clone();
    want.sort();
    want.reverse();
    assert_eq!(every, want);
}

#[test]
fn the_show_filters_tell_unread_reading_finished_favourite_and_new() {
    let s = shelf(&[
        // Never opened, added long ago.
        line("books/a.epub", "A", "Unread", "", "", ["", "", "100"]),
        line("books/b.epub", "B", "Reading", "", "", ["30", "o", "100"]),
        line("books/c.epub", "C", "Finished", "", "", ["100", "o", "100"]),
        line("books/d.epub", "D", "Loved", "", "", ["", "f", "100"]),
        // Added 5 days ago and never opened: new.
        line("books/e.epub", "E", "Fresh", "", "", ["", "", "19995"]),
        // Added 5 days ago, opened: not new.
        line("books/f.epub", "F", "Opened", "", "", ["10", "o", "19995"]),
    ]);
    let reads = s.reads();
    let shown = |show| {
        let mut q = ask();
        q.show = show;
        let (rows, end) = page(&reads, &q, 99).unwrap();
        assert_eq!(end.total as usize, rows.len());
        rows.into_iter().map(|r| r.title).collect::<Vec<_>>()
    };
    assert_eq!(shown(0).len(), 6);
    assert_eq!(shown(1), ["Fresh", "Loved", "Unread"]);
    assert_eq!(shown(2), ["Opened", "Reading"]);
    assert_eq!(shown(3), ["Finished"]);
    assert_eq!(shown(4), ["Loved"]);
    assert_eq!(shown(5), ["Fresh"]);
}

#[test]
fn words_author_series_tag_folder_and_day_narrow_the_books() {
    let tagged =
        "books/sci/b.epub\tBob\tDune\tArrakis\t1\tspace\u{1f}desert\t\t\t19990\t\t5\n".to_string();
    let s = shelf(&[
        line(
            "books/a.epub",
            "Ann Lee",
            "Sea Of Stories",
            "",
            "",
            ["", "", "19000"],
        ),
        tagged,
        line(
            "books/sci/c.epub",
            "Cat",
            "Neuromancer",
            "Sprawl",
            "1",
            ["", "", "19999"],
        ),
    ]);
    let reads = s.reads();
    let found = |edit: &dyn Fn(&mut wire::ServiceLibQuery<'static>)| {
        let mut q = ask();
        edit(&mut q);
        let (rows, end) = page(&reads, &q, 99).unwrap();
        assert_eq!(end.total as usize, rows.len());
        rows.into_iter().map(|r| r.title).collect::<Vec<_>>()
    };
    // Any case, any order, in title, author, series, tags or file name.
    assert_eq!(found(&|q| q.words = "SEA lee"), ["Sea Of Stories"]);
    assert_eq!(found(&|q| q.words = "desert arrakis"), ["Dune"]);
    assert_eq!(found(&|q| q.words = "c.epub"), ["Neuromancer"]);
    assert!(found(&|q| q.words = "sea dune").is_empty());
    assert_eq!(found(&|q| q.author = "Cat"), ["Neuromancer"]);
    assert_eq!(found(&|q| q.series = "Arrakis"), ["Dune"]);
    assert_eq!(found(&|q| q.tag = "space"), ["Dune"]);
    assert!(found(&|q| q.tag = "spa").is_empty(), "a whole tag");
    assert_eq!(found(&|q| q.folder = "books/sci"), ["Dune", "Neuromancer"]);
    assert_eq!(found(&|q| q.folder = "books/sci/"), ["Dune", "Neuromancer"]);
    assert!(
        found(&|q| q.folder = "books/sc").is_empty(),
        "a whole folder"
    );
    assert_eq!(found(&|q| q.added_since = 19_990), ["Dune", "Neuromancer"]);
}

#[test]
fn the_orders_are_author_series_added_last_first_and_recently_opened() {
    let s = shelf(&[
        line(
            "books/1.epub",
            "Zoe",
            "Alpha",
            "Two",
            "10",
            ["", "", "19000"],
        ),
        line("books/2.epub", "Amy", "Beta", "Two", "2", ["", "", "19500"]),
        line(
            "books/3.epub",
            "Amy",
            "Alpha",
            "One",
            "1",
            ["", "", "19900"],
        ),
        line("books/4.epub", "Max", "Gamma", "", "", ["", "", "19100"]),
    ]);
    // Read last: book 4, then book 1.
    s.storage
        .write(
            ".paperpaw/library/opened.txt",
            b"books/4.epub\nbooks/1.epub\n",
        )
        .unwrap();
    let reads = s.reads();
    let by = |sort| {
        let mut q = ask();
        q.sort = sort;
        page(&reads, &q, 99)
            .unwrap()
            .0
            .into_iter()
            .map(|r| format!("{}/{}", r.author, r.title))
            .collect::<Vec<_>>()
    };
    // A tie is the path: book 1 before book 3.
    assert_eq!(by(0), ["Zoe/Alpha", "Amy/Alpha", "Amy/Beta", "Max/Gamma"]);
    assert_eq!(by(1), ["Amy/Alpha", "Amy/Beta", "Max/Gamma", "Zoe/Alpha"]);
    // No series first, then "one", then "two" by number (2 before 10).
    assert_eq!(by(2), ["Max/Gamma", "Amy/Alpha", "Amy/Beta", "Zoe/Alpha"]);
    // Added last first.
    assert_eq!(by(3), ["Amy/Alpha", "Amy/Beta", "Max/Gamma", "Zoe/Alpha"]);
    // Opened rank, the books never opened after them by title.
    assert_eq!(by(4), ["Max/Gamma", "Zoe/Alpha", "Amy/Alpha", "Amy/Beta"]);
}

#[test]
fn a_named_generation_that_is_not_the_catalogs_is_state_and_zero_is_the_current_one() {
    let s = many();
    let reads = s.reads();
    let generation = page(&reads, &ask(), 1).unwrap().1.generation;
    let mut q = ask();
    q.generation = generation;
    assert!(page(&reads, &q, 1).is_ok());
    q.generation = generation ^ 0x5555;
    assert_eq!(page(&reads, &q, 1).err(), Some(ERR_STATE));
    // The catalog changed: the old generation is stale.
    paperpaw_sdk::library::catalog::forget(&*s.storage, "books/00.epub").unwrap();
    q.generation = generation;
    assert_eq!(page(&reads, &q, 1).err(), Some(ERR_STATE));
    let (_, end) = page(&reads, &ask(), 1).unwrap();
    assert_eq!(end.total, 44);
    assert_ne!(end.generation, generation);
}

#[test]
fn a_book_a_pending_delete_holds_is_not_listed_or_counted() {
    let s = many();
    let mut rows = 0;
    let end = s
        .reads()
        .query_hiding(&ask(), &|path| path == "books/00.epub", &mut |_| {
            rows += 1;
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!((rows, end.total), (32, 44));
}

#[test]
fn the_archive_folder_is_left_out_when_asked() {
    let s = shelf(&[
        line("books/a.epub", "A", "Here", "", "", ["", "", "1"]),
        line("books/old/b.epub", "B", "Archived", "", "", ["", "", "1"]),
    ]);
    let reads = s.reads();
    let mut q = ask();
    assert_eq!(
        page_archived(&reads, &q, "books/old/", 9).unwrap().1.total,
        2
    );
    // The flag names it; without a folder nothing is left out.
    q.flags = 0b10;
    let (rows, end) = page_archived(&reads, &q, "books/old/", 9).unwrap();
    assert_eq!((titles(&rows), end.total), (["Here"].to_vec(), 1));
    assert_eq!(page_archived(&reads, &q, "", 9).unwrap().1.total, 2);
}

#[test]
fn a_book_by_the_handle_a_query_gave_is_that_book() {
    let s = many();
    let reads = s.reads();
    let (rows, end) = page(&reads, &ask(), 3).unwrap();
    for row in rows {
        let mut seen = None;
        reads
            .book(
                BookRef::Key {
                    key: row.key,
                    generation: end.generation,
                },
                &mut |record| {
                    let Record::ServiceLibBook(b) = record else {
                        panic!("a book")
                    };
                    seen = Some(b.title.to_string());
                },
            )
            .unwrap();
        assert_eq!(seen.unwrap(), row.title);
    }
}

#[test]
fn an_empty_or_missing_catalog_has_no_books() {
    let s = Shelf {
        storage: Rc::new(MemoryStorage::new()),
        env: Rc::new(Env {
            walking: Cell::new(None),
            today: Cell::new(None),
        }),
        at: Vec::new(),
    };
    let (rows, end) = page(&s.reads(), &ask(), 9).unwrap();
    assert!(rows.is_empty());
    assert_eq!((end.total, end.next), (0, 0));
}

#[test]
fn a_sort_key_is_cut_at_96_bytes_and_a_tie_goes_by_path() {
    let long = |tail: &str| format!("{}{tail}", "x".repeat(96));
    let s = shelf(&[
        line("books/b.epub", "A", &long("zzz"), "", "", ["", "", "1"]),
        line("books/a.epub", "A", &long("aaa"), "", "", ["", "", "1"]),
        line("books/c.epub", "A", "short", "", "", ["", "", "1"]),
    ]);
    let (rows, _) = page(&s.reads(), &ask(), 9).unwrap();
    // "short" first; the two long titles agree to the cut: their path
    // decides (a before b), not the tail.
    let keys: Vec<u64> = rows.iter().map(|r| u64::from(r.key)).collect();
    assert_eq!(keys, [s.at[2], s.at[1], s.at[0]]);
}
