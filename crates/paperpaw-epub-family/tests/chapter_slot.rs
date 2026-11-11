//! A chapter converted by this very book but not marked complete in the
//! cache (`<n>.end` absent: a picture found no room to be decoded) is read
//! again from what is there while the book is open, and converted again
//! the next time it is opened.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{blocks, book};
use paperpaw_epub_family::EpubDocument;
use paperpaw_sdk::document::Document;
use paperpaw_sdk::{MemoryStorage, Storage};

/// The cache files of `storage` named `suffix` (they have no extension: the
/// chapter is the folder, `sections/0/end`).
fn files_ending(storage: &dyn Storage, suffix: &str) -> Vec<String> {
    fn walk(storage: &dyn Storage, dir: &str, suffix: &str, out: &mut Vec<String>) {
        let mut found = Vec::new();
        let _ = storage.list(dir, &mut |entry| {
            found.push((entry.name.to_string(), entry.is_dir));
        });
        for (name, is_dir) in found {
            let path = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                walk(storage, &path, suffix, out);
            } else if name == suffix {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(storage, "", suffix, &mut out);
    out
}

fn storage() -> MemoryStorage {
    let bodies = ["<p>The first chapter has some text.</p>".to_string()];
    MemoryStorage::new().with("book.epub", book(&bodies, &[]))
}

fn texts(doc: &dyn Document, storage: &dyn Storage) -> Vec<String> {
    blocks(doc, storage)
        .into_iter()
        .map(|(_, block)| block.text)
        .collect()
}

#[test]
fn a_chapter_of_the_right_length_without_its_mark_is_converted_again_in_a_new_book() {
    let storage = storage();
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    let first = texts(&doc, &storage);
    assert!(!first.is_empty());
    let marks = files_ending(&storage, "end");
    assert_eq!(marks.len(), 1, "{marks:?}");
    // The mark goes, the text stays at its length.
    storage.remove(&marks[0]).unwrap();
    assert!(!files_ending(&storage, "txt").is_empty());
    let again = EpubDocument::open(&storage, "book.epub").unwrap();
    assert_eq!(texts(&again, &storage), first);
    // Converted again: marked complete once more.
    assert_eq!(files_ending(&storage, "end"), marks);
}

#[test]
fn the_same_book_reads_a_chapter_without_its_mark_without_converting_it_again() {
    let storage = storage();
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    let first = texts(&doc, &storage);
    let marks = files_ending(&storage, "end");
    storage.remove(&marks[0]).unwrap();
    // What this book converted is read on, not converted again.
    assert_eq!(texts(&doc, &storage), first);
    assert!(files_ending(&storage, "end").is_empty());
}
