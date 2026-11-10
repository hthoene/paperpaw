#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Who imported a book: kept by the book's path, never of another book.
extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;
use std::string::ToString;

const BOOK: &str = "Books/feed/issue-1.epub";

#[test]
fn the_importer_is_kept_for_the_path_and_replaced_by_the_next_import() {
    let storage = MemoryStorage::new();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    set(&storage, BOOK, "news").unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(true));
    // Another plugin is not the importer; a prefix of the id is not it either.
    assert_eq!(is_by(&storage, BOOK, "wallabag"), Ok(false));
    assert_eq!(is_by(&storage, BOOK, "new"), Ok(false));
    // Nor is another path.
    assert_eq!(
        is_by(&storage, "Books/feed/issue-2.epub", "news"),
        Ok(false)
    );
    set(&storage, BOOK, "wallabag").unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    assert_eq!(is_by(&storage, BOOK, "wallabag"), Ok(true));
}

#[test]
fn a_record_is_removed_and_removing_none_is_no_error() {
    let storage = MemoryStorage::new();
    remove(&storage, BOOK).unwrap();
    set(&storage, BOOK, "news").unwrap();
    remove(&storage, BOOK).unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    set(&storage, BOOK, "news").unwrap();
    clear(&storage, BOOK);
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
}

#[test]
fn an_id_no_plugin_has_is_refused_and_nothing_is_written() {
    let storage = MemoryStorage::new();
    for id in ["", "a\nb", &"x".repeat(97)] {
        assert_eq!(set(&storage, BOOK, id), Err(ERR_INVALID), "{id:?}");
    }
    assert!(storage.size(&file(BOOK).unwrap()).is_err());
    // The longest id is kept whole.
    let longest = "y".repeat(96);
    set(&storage, BOOK, &longest).unwrap();
    assert_eq!(is_by(&storage, BOOK, &longest), Ok(true));
}

#[test]
fn a_record_that_is_no_id_is_not_the_importers() {
    let storage = MemoryStorage::new();
    storage.write(&file(BOOK).unwrap(), &[0xff, 0xfe]).unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    storage
        .write(&file(BOOK).unwrap(), "z".repeat(200).as_bytes())
        .unwrap();
    assert_eq!(is_by(&storage, BOOK, &"z".repeat(200)), Ok(false));
    let _ = "x".to_string();
}

#[test]
fn the_record_is_named_by_the_path_so_a_move_leaves_none_for_the_new_one() {
    assert_ne!(
        file(BOOK).unwrap(),
        file("Books/moved/issue-1.epub").unwrap()
    );
    assert!(
        file(BOOK)
            .unwrap()
            .starts_with(".paperpaw/library/imported/")
    );
}
