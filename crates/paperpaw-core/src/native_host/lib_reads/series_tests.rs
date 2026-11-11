#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 6 `next_in_series` over catalogs in memory.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::Env;
use crate::native_host::lib_service::LibQuery;
use crate::native_host::registrations::Record;
use alloc::rc::Rc;
use alloc::string::ToString;
use core::cell::Cell;
use paperpaw_sdk::MemoryStorage;
use std::format;
use std::string::String;

const HEADER: &str = "\tcatalog\t2\t1\n";

/// One catalog and where each book's line starts.
struct Shelf {
    reads: CatalogReads,
    at: std::vec::Vec<u64>,
}

/// `(title, series, number, progress)` of each book, in file order.
fn shelf(books: &[(&str, &str, &str, &str)]) -> Shelf {
    let mut text = String::from(HEADER);
    let mut at = std::vec::Vec::new();
    for (n, (title, series, number, progress)) in books.iter().enumerate() {
        at.push(text.len() as u64);
        text.push_str(&format!(
            "books/{n}.epub\tAnn\t{title}\t{series}\t{number}\t\t{progress}\t\t1\t\t5\n"
        ));
    }
    let storage =
        Rc::new(MemoryStorage::new().with(".paperpaw/library/catalog.txt", text.into_bytes()));
    let env = Rc::new(Env {
        walking: Cell::new(None),
        today: Cell::new(Some(20_000)),
    });
    Shelf {
        reads: CatalogReads::new(storage, env),
        at,
    }
}

impl Shelf {
    /// The title of the next book after the one at `index`, or the status.
    fn next(&self, index: usize) -> Result<(String, u32), i32> {
        let mut got = None;
        self.reads.next_in_series(
            BookRef::Path(&format!("books/{index}.epub")),
            &mut |record| {
                let Record::ServiceLibEntry(e) = record else {
                    panic!("an entry")
                };
                got = Some((e.title.to_string(), e.key));
            },
        )?;
        Ok(got.unwrap())
    }
}

#[test]
fn the_lowest_unread_book_of_the_series_is_next_whatever_book_is_asked_about() {
    let s = shelf(&[
        ("S1", "Saga", "1", "50"),
        ("S3", "Saga", "3", ""),
        ("S10", "Saga", "10", ""),
        ("S2", "Saga", "2", ""),
        ("T1", "Tale", "1", ""),
        ("Alone", "", "", ""),
    ]);
    for asked in 0..4 {
        let (title, key) = s.next(asked).unwrap();
        assert_eq!(title, "S2", "asked about {asked}");
        assert_eq!(u64::from(key), s.at[3]);
    }
    // Another series has its own.
    assert_eq!(s.next(4).unwrap().0, "T1");
    // No series: none.
    assert_eq!(s.next(5), Err(ERR_NOT_FOUND));
}

#[test]
fn a_series_all_of_whose_books_were_opened_has_no_next_and_a_tie_is_the_first_line() {
    let s = shelf(&[("A", "Saga", "1", "10"), ("B", "Saga", "2", "100")]);
    assert_eq!(s.next(0), Err(ERR_NOT_FOUND));
    let s = shelf(&[
        ("First", "Saga", "2", ""),
        ("Second", "Saga", "2", ""),
        ("Third", "Saga", "x", ""),
    ]);
    assert_eq!(s.next(2).unwrap().0, "First");
}

#[test]
fn an_unknown_book_is_not_found_and_a_book_a_pending_delete_holds_is_passed_over() {
    let s = shelf(&[("A", "Saga", "1", ""), ("B", "Saga", "2", "")]);
    assert_eq!(s.next(9), Err(ERR_NOT_FOUND));
    let mut got = None;
    s.reads
        .next_in_series_hiding(
            BookRef::Path("books/1.epub"),
            &|path| path == "books/0.epub",
            &mut |record| {
                let Record::ServiceLibEntry(e) = record else {
                    panic!()
                };
                got = Some(e.title.to_string());
            },
        )
        .unwrap();
    assert_eq!(got.as_deref(), Some("B"));
}
