//! The library service: a book's details (`books/<hash>.ini`) come back
//! with any title, authors, tags and fields; the lists of books opened
//! and favourites keep odd paths and survive hostile files; reading
//! events are recorded; listings page through any folder in one order;
//! the catalog reads back what it wrote.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::borrow::Cow;

use paperpaw_sdk::builtin::events::Reading;
use paperpaw_sdk::library::{self, BookData, BookDataKind};
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

const PATH: &str = "books/a.epub";

fn reading(path: &str, title: &str, progress: f32) -> Reading {
    Reading::new(path.into(), title.into(), progress, 0, 0, String::new())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]


    /// Favourites keep any path (spaces around, `#`, `=`, Unicode) and
    /// come newest first, each once. Paths starting with a byte order
    /// mark are left to `a_first_favourite_starting_with_a_bom_is_kept`.
    #[test]
    fn favourites_keep_odd_paths(paths in proptest::collection::vec("[^\n\r\u{feff}][^\n\r]{0,29}", 1..12)) {
        let storage = MemoryStorage::new();
        for path in &paths {
            library::set_favourite(&storage, path, true);
        }
        let mut expected: Vec<String> = Vec::new();
        for path in paths.iter().rev() {
            if !expected.contains(path) {
                expected.push(path.clone());
            }
        }
        prop_assert_eq!(library::favourites(&storage), expected.clone());
        library::set_favourite(&storage, &paths[0], false);
        expected.retain(|p| *p != paths[0]);
        prop_assert_eq!(library::favourites(&storage), expected);
    }

    /// Whatever bytes the lists hold, reading them does not panic.
    #[test]
    fn any_list_file_is_read(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let storage = MemoryStorage::new()
            .with(".paperpaw/library/opened.txt", bytes.clone())
            .with(".paperpaw/library/favourites.txt", bytes);
        for path in library::opened_books(&storage).iter().chain(&library::favourites(&storage)) {
            prop_assert!(!path.is_empty());
        }
    }

    /// Whatever bytes a book's record holds, reading it does not panic.
    #[test]
    fn any_record_is_read(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let storage = MemoryStorage::new()
            .with(PATH, b"12345".to_vec())
            .with(".paperpaw/library/details/0c/0c6afbf2778c9bbf.ini", bytes);
        if let Some(book) = library::book(&storage, PATH) {
            prop_assert_eq!(book.size, 5);
            prop_assert!(book.progress.is_none_or(|p| p <= 100));
        }
    }



}

#[test]
fn record_names_its_file_by_the_path_hash() {
    // As the testdata of version 1 has it (`docs/data-files.md`).
    assert_eq!(library::key("books/a.epub"), "0c6afbf2778c9bbf");
    assert_eq!(library::key("").len(), 16);
}

#[test]
fn favourites_with_duplicates_and_blank_lines_are_read() {
    let storage = MemoryStorage::new().with(
        ".paperpaw/library/favourites.txt",
        "a.epub\n\nb.epub\na.epub\r\n  spaced.epub  \n",
    );
    // As the file says, blank lines left out.
    assert_eq!(
        library::favourites(&storage),
        ["a.epub", "b.epub", "a.epub", "  spaced.epub  "]
    );
    // A change of a book keeps it once.
    library::set_favourite(&storage, "a.epub", true);
    assert_eq!(
        library::favourites(&storage),
        ["a.epub", "b.epub", "  spaced.epub  "]
    );
    library::set_favourite(&storage, "  spaced.epub  ", false);
    assert_eq!(library::favourites(&storage), ["a.epub", "b.epub"]);
}

#[test]
fn favourites_are_capped_and_the_oldest_go() {
    let storage = MemoryStorage::new();
    for i in 0..510 {
        library::set_favourite(&storage, &format!("b/{i}.epub"), true);
    }
    let list = library::favourites(&storage);
    assert_eq!(list.len(), 500);
    assert_eq!(list[0], "b/509.epub");
    assert_eq!(list[499], "b/10.epub");
}

#[test]
fn opened_books_are_recorded_most_recent_first_and_capped() {
    let h = common::Harness::new();
    let env = h.env();
    for i in 0..105 {
        library::record(&env, &reading(&format!("b/{i}.txt"), "", 0.0), true);
    }
    library::record(&env, &reading("b/50.txt", "", 0.0), true);
    let opened = library::opened_books(&h.storage);
    assert_eq!(opened.len(), 100);
    assert_eq!(opened[0], "b/50.txt");
    assert_eq!(opened[1], "b/104.txt");
    assert_eq!(opened.iter().filter(|p| *p == "b/50.txt").count(), 1);
    assert!(!opened.contains(&"b/4.txt".to_string()));
}

#[test]
fn reading_progress_is_rounded_and_clamped() {
    let mut h = common::Harness::new();
    h.storage = MemoryStorage::new().with("b/a.txt", "hello");
    let env = h.env();
    for (progress, kept) in [
        (12.4, 12),
        (12.5, 13),
        (99.6, 100),
        (250.0, 100),
        (-3.0, 0),
        (f32::NAN, 0),
        (f32::INFINITY, 100),
    ] {
        library::record(&env, &reading("b/a.txt", "", progress), false);
        let book = library::book(&h.storage, "b/a.txt").unwrap();
        assert_eq!(book.progress, Some(kept), "{progress}");
    }
    // Not opened: no place among the opened.
    assert!(library::opened_books(&h.storage).is_empty());
}

#[test]
fn records_written_by_an_older_details_version_are_read_again() {
    let storage = MemoryStorage::new().with(PATH, "12345").with(
        ".paperpaw/library/details/0c/0c6afbf2778c9bbf.ini",
        "[book]\nsize = 5\nindexed = on\nversion = 4\ntitle = New\n",
    );
    assert!(library::book(&storage, PATH).unwrap().indexed);
    for older in ["version = 3", "version = x", ""] {
        let text = format!("[book]\nsize = 5\nindexed = on\n{older}\ntitle = Old\n");
        let storage = MemoryStorage::new()
            .with(PATH, "12345")
            .with(".paperpaw/library/details/0c/0c6afbf2778c9bbf.ini", text);
        let book = library::book(&storage, PATH).unwrap();
        assert!(!book.indexed, "{older:?}");
        assert_eq!(book.title.as_deref(), Some("Old"));
    }
}

#[test]
fn a_record_without_a_size_belongs_to_an_empty_file_only() {
    let storage = MemoryStorage::new().with(PATH, "").with(
        ".paperpaw/library/details/0c/0c6afbf2778c9bbf.ini",
        "[book]\ntitle = T\n",
    );
    assert_eq!(
        library::book(&storage, PATH).unwrap().title.as_deref(),
        Some("T")
    );
    assert_eq!(library::book(&storage, "books/missing.epub"), None);
}

#[test]
fn book_data_entries_are_named_by_sixteen_small_hex_digits() {
    let data = BookData::new(
        Cow::Borrowed("cache"),
        Cow::Borrowed("epub-"),
        BookDataKind::default(),
    );
    let key = "0123456789abcdef";
    assert_eq!(data.key_of(&format!("epub-{key}")), Some((key, "")));
    assert_eq!(data.key_of(&format!("epub-{key}.ini")), Some((key, ".ini")));
    for name in [
        "0123456789abcdef".to_string(),
        format!("epub-{}", key.to_uppercase()),
        "epub-0123456789abcde".into(),
        format!("epub-{key}x"),
        format!("epub-{key}-1"),
        "epub-0123456789abcdeé".into(),
        "epub-日本語日本語".into(),
        "epub-".into(),
        String::new(),
    ] {
        assert_eq!(data.key_of(&name), None, "{name}");
    }
}

#[test]
fn opened_list_of_older_versions_is_taken_over_once() {
    let storage = MemoryStorage::new()
        .with(".paperpaw/plugins/files/opened.txt", "old.epub")
        .with(".paperpaw/plugins/files/catalog.txt", "x");
    library::take_over_old(&storage);
    assert_eq!(library::opened_books(&storage), ["old.epub"]);
    assert!(!storage.exists(".paperpaw/plugins/files/catalog.txt"));
    // Once there, older files stay where they are.
    let storage = storage.with(".paperpaw/plugins/files/favourites.txt", "f.epub");
    library::take_over_old(&storage);
    assert!(library::favourites(&storage).is_empty());
}

/// One line of the favourites that is not UTF-8 (a damaged card) makes
/// the list unreadable, but the next change does not write it anew
/// without the other favourites: the file is left as it is.
#[test]
fn a_damaged_line_does_not_lose_the_other_favourites() {
    let damaged = b"a.epub\nb\xff.epub\nc.epub";
    let storage = MemoryStorage::new().with(".paperpaw/library/favourites.txt", damaged.to_vec());
    library::set_favourite(&storage, "d.epub", true);
    library::set_favourite(&storage, "a.epub", false);
    assert_eq!(
        storage.get(".paperpaw/library/favourites.txt").as_deref(),
        Some(&damaged[..])
    );
}

/// The lists are written without a byte order mark but read with one
/// left out, so a path first in the list that starts with U+FEFF (a
/// valid file name) loses it and no longer names its book.
#[test]
fn a_first_favourite_starting_with_a_bom_is_kept() {
    let storage = MemoryStorage::new();
    library::set_favourite(&storage, "\u{feff}a.epub", true);
    assert_eq!(library::favourites(&storage), ["\u{feff}a.epub"]);
}
