#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 22 `fields` over a book's details on a card in memory.
extern crate std;
use super::*;
use crate::native_host::lib_reads::CatalogEnv;
use crate::native_host::lib_reads::tests::Env;
use crate::native_host::lib_service::LibQuery;
use alloc::rc::Rc;
use alloc::string::ToString;
use core::cell::Cell;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::MemoryStorage;
use std::string::String;

const HEADER: &str = "\tcatalog\t2\t1\n";

struct Titled;
impl CatalogEnv for Titled {
    fn walking(&self) -> Option<u32> {
        None
    }
    fn today(&self) -> Option<u32> {
        None
    }
    fn field_title(&self, id: &str, visit: &mut dyn FnMut(&str)) {
        if id == "stars.rating" {
            visit("Rating");
        }
    }
}

fn card() -> (Rc<MemoryStorage>, CatalogReads) {
    let storage = Rc::new(
        MemoryStorage::new()
            .with(
                ".paperpaw/library/catalog.txt",
                std::format!("{HEADER}books/a.epub\tAnn\tA\t\t\t\t\t\t1\t\t10\n").into_bytes(),
            )
            .with("books/a.epub", std::vec![1_u8; 10]),
    );
    let env = Rc::new(Env {
        walking: Cell::new(None),
        today: Cell::new(None),
    });
    let reads = CatalogReads::new(storage.clone(), env);
    (storage, reads)
}

fn set(storage: &MemoryStorage, id: &str, value: &str) {
    library::try_set_field(storage, "books/a.epub", id, value).unwrap();
}

type Rows3 = std::vec::Vec<(String, String, String)>;

fn page(
    reads: &CatalogReads,
    cursor: u32,
    generation: u32,
    room: usize,
) -> Result<(Rows3, RowsEnd), i32> {
    let mut rows = Rows3::new();
    let end = reads.fields(
        BookRef::Path("books/a.epub"),
        cursor,
        generation,
        &mut |record| {
            if rows.len() == room {
                return ControlFlow::Break(());
            }
            let Record::ServiceLibField(f) = record else {
                panic!("a field")
            };
            rows.push((f.id.to_string(), f.title.to_string(), f.value.to_string()));
            ControlFlow::Continue(())
        },
    )?;
    Ok((rows, end))
}

#[test]
fn the_fields_of_every_plugin_come_by_plugin_then_name_with_a_title() {
    let (storage, reads) = card();
    for (id, value) in [
        ("zed.note", "z"),
        ("a.x", "1"),
        ("a-b.x", "2"),
        ("a.b", "3"),
        ("stars.rating", "5"),
    ] {
        set(&storage, id, value);
    }
    let (rows, end) = page(&reads, 0, 0, 99).unwrap();
    let ids: std::vec::Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    // The plugin's id first ("a" before "a-b"), then the name.
    assert_eq!(ids, ["a.b", "a.x", "a-b.x", "stars.rating", "zed.note"]);
    assert_eq!((end.total, end.next), (5, 5));
    assert_ne!(end.generation, 0);
    // No title declared: the field's name.
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_str()), ("b", "3"));
}

#[test]
fn a_title_the_plugin_declared_is_the_rows_title() {
    let (storage, _) = card();
    set(&storage, "stars.rating", "5");
    let reads = CatalogReads::new(storage, Rc::new(Titled));
    let (rows, _) = page(&reads, 0, 0, 9).unwrap();
    assert_eq!(rows[0].1, "Rating");
}

#[test]
fn pages_follow_the_cursor_and_a_changed_field_makes_the_generation_stale() {
    let (storage, reads) = card();
    for (id, value) in [("a.one", "1"), ("a.two", "2"), ("a.three", "3")] {
        set(&storage, id, value);
    }
    // One row to a page: the end names where the next starts.
    let (first, end) = page(&reads, 0, 0, 1).unwrap();
    assert_eq!((first[0].0.as_str(), end.total, end.next), ("a.one", 3, 1));
    let generation = end.generation;
    let (second, end) = page(&reads, 1, generation, 2).unwrap();
    assert_eq!(
        (second[0].0.as_str(), second[1].0.as_str(), end.next),
        ("a.three", "a.two", 3)
    );
    // Past the last: empty.
    let (none, end) = page(&reads, 7, generation, 2).unwrap();
    assert!(none.is_empty());
    assert_eq!((end.total, end.next), (3, 3));
    // A change: the generation moves.
    set(&storage, "a.two", "changed");
    assert_eq!(page(&reads, 1, generation, 2).err(), Some(ERR_STATE));
    let (_, again) = page(&reads, 0, 0, 1).unwrap();
    assert_ne!(again.generation, generation);
    // Removing a field (an empty value) moves it too, and drops the row.
    set(&storage, "a.two", "");
    let (rows, end) = page(&reads, 0, 0, 9).unwrap();
    assert_eq!((rows.len(), end.total), (2, 2));
}

#[test]
fn a_book_without_fields_has_none_and_one_that_is_not_there_is_not_found() {
    let (_, reads) = card();
    let (rows, end) = page(&reads, 0, 0, 9).unwrap();
    assert!(rows.is_empty());
    assert_eq!((end.total, end.next), (0, 0));
    assert_ne!(end.generation, 0);
    let gone = reads.fields(BookRef::Path("books/zz.epub"), 0, 0, &mut |_| {
        ControlFlow::Continue(())
    });
    assert_eq!(gone.err(), Some(ERR_NOT_FOUND));
}
