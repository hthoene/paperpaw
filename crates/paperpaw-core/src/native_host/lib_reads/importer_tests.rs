#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Who imported a book: kept by the book's path and the file's fingerprint,
//! never of another book or of a file put there since.
extern crate std;
use super::*;
use paperpaw_platform::StorageError;
use paperpaw_sdk::MemoryStorage;
use std::vec;

const BOOK: &str = "Books/feed/issue-1.epub";

fn card() -> MemoryStorage {
    MemoryStorage::new().with(BOOK, vec![1_u8; 3000])
}

#[test]
fn the_importer_is_kept_for_the_path_and_replaced_by_the_next_import() {
    let storage = card();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    set(&storage, BOOK, "news").unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(true));
    // Another plugin is not the importer; a prefix of the id is not it either.
    assert_eq!(is_by(&storage, BOOK, "wallabag"), Ok(false));
    assert_eq!(is_by(&storage, BOOK, "new"), Ok(false));
    // Nor is another path.
    storage
        .write("Books/feed/issue-2.epub", &[1_u8; 3000])
        .unwrap();
    assert_eq!(
        is_by(&storage, "Books/feed/issue-2.epub", "news"),
        Ok(false)
    );
    set(&storage, BOOK, "wallabag").unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    assert_eq!(is_by(&storage, BOOK, "wallabag"), Ok(true));
}

#[test]
fn a_file_put_there_since_is_not_the_importers() {
    let storage = card();
    set(&storage, BOOK, "news").unwrap();
    // The user deletes the book on a computer and copies a file of their own
    // to the same name: another size, and then the same size with other bytes.
    storage.write(BOOK, &[2_u8; 4096]).unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    storage.write(BOOK, &[9_u8; 3000]).unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    // The plugin's own replacement, imported again, is its own once more.
    set(&storage, BOOK, "news").unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(true));
    // The file gone: not the importer's either.
    storage.remove(BOOK).unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
}

#[test]
fn a_record_is_removed_and_removing_none_is_no_error() {
    let storage = card();
    remove(&storage, BOOK).unwrap();
    set(&storage, BOOK, "news").unwrap();
    remove(&storage, BOOK).unwrap();
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
    set(&storage, BOOK, "news").unwrap();
    clear(&storage, BOOK);
    assert_eq!(is_by(&storage, BOOK, "news"), Ok(false));
}

#[test]
fn an_id_no_plugin_has_is_refused_and_a_file_that_cannot_be_read_is_the_cards_error() {
    let storage = card();
    for id in ["", "a\nb", &"x".repeat(97)] {
        assert_eq!(set(&storage, BOOK, id), Err(ERR_INVALID), "{id:?}");
    }
    assert!(storage.size(&file(BOOK).unwrap()).is_err());
    // The longest id is kept whole.
    let longest = "y".repeat(96);
    set(&storage, BOOK, &longest).unwrap();
    assert_eq!(is_by(&storage, BOOK, &longest), Ok(true));
    // No file to tell it by: nothing is written.
    assert_eq!(set(&storage, "Books/none.epub", "news"), Err(ERR_IO));
    assert_eq!(
        storage.size(&file("Books/none.epub").unwrap()),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_record_that_is_no_id_and_fingerprint_is_not_the_importers() {
    let storage = card();
    let record = file(BOOK).unwrap();
    for bytes in [
        vec![0xff, 0xfe],
        "news\n".as_bytes().to_vec(),
        "news\nnot a fingerprint\n".as_bytes().to_vec(),
        "z".repeat(300).into_bytes(),
    ] {
        storage.write(&record, &bytes).unwrap();
        assert_eq!(is_by(&storage, BOOK, "news"), Ok(false), "{bytes:?}");
    }
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
