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
use paperpaw_sdk::document::{DocumentError, ImageRef};
use paperpaw_sdk::library::listing::{Entry, Listing, UNRANKED};
use paperpaw_sdk::library::{self, Book, BookData, BookDataKind, catalog};
use paperpaw_sdk::{FormatSpec, Formats, Id, MemoryStorage, Storage};
use proptest::prelude::*;

const PATH: &str = "books/a.epub";

fn txt_formats() -> Formats {
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.txt"),
            "TXT",
            vec![Cow::Borrowed("txt")],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    formats
}

/// Text as a book's detail: any but carriage returns, which the ini
/// format leaves out of quoted values.
fn detail() -> impl Strategy<Value = String> {
    "[^\r]{0,24}"
}

fn any_book() -> impl Strategy<Value = Book> {
    (
        (
            any::<u64>(),
            any::<bool>(),
            proptest::option::of(detail()),
            proptest::option::of(detail()),
            proptest::option::of(detail()),
            proptest::option::of(detail()),
        ),
        proptest::collection::vec(detail(), 0..4),
        proptest::option::of((detail(), any::<u32>(), any::<u32>())),
        (
            proptest::option::of(0u8..=100),
            proptest::option::of(any::<u32>()),
        ),
        proptest::collection::btree_map("[a-z]{1,6}\\.[a-z-]{1,6}", detail(), 0..4),
    )
        .prop_map(
            |(
                (size, indexed, title, author, series, series_index),
                tags,
                cover,
                (progress, added),
                fields,
            )| {
                Book::default()
                    .size(size)
                    .indexed(indexed)
                    .title(title)
                    .author(author)
                    .series(series)
                    .series_index(series_index)
                    .tags(tags)
                    .cover(cover.map(|(path, width, height)| ImageRef {
                        path,
                        width,
                        height,
                    }))
                    .progress(progress)
                    .added(added)
                    .fields(fields.into_iter().collect())
            },
        )
}

fn reading(path: &str, title: &str, progress: f32) -> Reading {
    Reading::new(path.into(), title.into(), progress, 0, 0, String::new())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Whatever a book's details hold (quotes, `=`, `#`, brackets, line
    /// breaks, spaces around, any Unicode), they come back as saved.
    #[test]
    fn book_details_come_back(book in any_book()) {
        let storage = MemoryStorage::new();
        library::save(&storage, PATH, &book).unwrap();
        let read = library::book_sized(&storage, PATH, book.size);
        prop_assert_eq!(read, Some(book.clone()));
        if book.size != u64::MAX {
            prop_assert_eq!(library::book_sized(&storage, PATH, book.size + 1), None);
        }
    }

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
            .with(".paperpaw/library/books/0c6afbf2778c9bbf.ini", bytes);
        if let Some(book) = library::book(&storage, PATH) {
            prop_assert_eq!(book.size, 5);
            prop_assert!(book.progress.is_none_or(|p| p <= 100));
        }
    }

    /// A file's name made readable is made so once: doing it again
    /// changes nothing, and it never grows a separator.
    #[test]
    fn readable_names_are_settled(stem in "[a-zA-Z0-9 ._-]{0,20}") {
        let once = library::readable_name(&stem).into_owned();
        let twice = library::readable_name(&once).into_owned();
        prop_assert_eq!(&twice, &once);
        if once != stem {
            prop_assert!(!once.contains(['-', '_', '.']));
        }
    }

    /// The pages of a folder, walked forward, backward or jumped to, are
    /// all its entries in one order: folders first, then names without
    /// regard to case.
    #[test]
    fn listings_page_through_every_entry_in_one_order(
        files in proptest::collection::btree_set("[a-zA-Z0-9 äÖ]{1,6}", 0..20),
        folders in proptest::collection::btree_set("[a-zA-Z]{1,5}", 0..5),
        rows in 1usize..6,
    ) {
        let mut storage = MemoryStorage::new();
        for name in &files {
            storage = storage.with(&format!("books/{name}.txt"), "");
        }
        for name in &folders {
            storage = storage.with(&format!("books/{name}_dir/x.txt"), "");
        }
        storage = storage.with("books/skipped.jpg", "");
        let formats = txt_formats();
        let listing = Listing::new(&storage, &formats, "books", false, None);
        let key = |n: &str| (n.to_lowercase(), n.to_string());
        let mut expected_folders: Vec<String> = folders.iter().map(|n| format!("{n}_dir")).collect();
        expected_folders.sort_by_key(|n| key(n));
        let mut expected_files: Vec<String> = files.iter().map(|n| format!("{n}.txt")).collect();
        expected_files.sort_by_key(|n| key(n));
        let expected: Vec<String> = expected_folders.into_iter().chain(expected_files).collect();

        let len = listing.count();
        prop_assert_eq!(len, expected.len());
        let pages = len.div_ceil(rows);
        let mut walked = Vec::new();
        let mut page = listing.after(None, rows);
        for p in 0..pages {
            let jumped = listing.page(p, rows, len);
            prop_assert_eq!(&jumped, &page, "page {}", p);
            walked.extend(page.iter().map(|e| e.name.clone()));
            page = listing.after(page.last(), rows);
        }
        prop_assert!(page.is_empty());
        prop_assert_eq!(&walked, &expected);
        for (i, name) in expected.iter().enumerate() {
            let entry = Entry::new(name.clone(), name.ends_with("_dir"), UNRANKED, 0);
            prop_assert_eq!(listing.rank(&entry), i);
            // First on its page: `the_page_of_an_entry_first_on_its_page_holds_it`.
            if i % rows != 0 {
                let (first, page) = listing.page_of(&entry, rows);
                prop_assert_eq!(first, i - i % rows);
                prop_assert!(page.iter().any(|e| e.name == *name));
            }
            let range: Vec<String> = listing.range(i, 2).into_iter().map(|e| e.name).collect();
            prop_assert_eq!(&range[..], &expected[i..(i + 2).min(len)]);
        }
        if let Some(last) = expected.last() {
            let tail = Entry::new(last.clone(), last.ends_with("_dir"), UNRANKED, 0);
            let before: Vec<String> = listing.before(Some(&tail), 3).into_iter().map(|e| e.name).collect();
            prop_assert_eq!(&before[..], &expected[len.saturating_sub(4)..len - 1]);
        }
    }

    /// Records of the catalog come back as written, whatever their text,
    /// also where the file is read in pieces that cut characters.
    #[test]
    fn catalog_records_come_back(
        books in proptest::collection::vec(
            ("[a-zß日]{1,8}", "[^\t\n\r\u{1f}]{0,40}", "[^\t\n\r\u{1f}]{0,40}", proptest::collection::vec("[a-zé]{1,6}", 0..3)),
            1..60,
        )
    ) {
        let storage = MemoryStorage::new();
        let records: Vec<catalog::Record> = books
            .iter()
            .enumerate()
            .map(|(i, (name, title, author, tags))| {
                let book = Book::default().title(format!("T{title}T")).author(format!("A{author}A")).series(String::from("S")).series_index(String::from("1")).tags(tags.clone());
                catalog::Record::of(&format!("books/{i}{name}.txt"), &book)
            })
            .collect();
        catalog::write(&storage, &records).unwrap();
        let mut read = Vec::new();
        catalog::each(&storage, &mut |r| read.push(r));
        prop_assert_eq!(read, records);
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
fn a_title_from_the_reader_is_kept_until_the_book_is_read() {
    let mut h = common::Harness::new();
    h.storage = MemoryStorage::new().with("b/a.txt", "hello");
    let env = h.env();
    library::record(&env, &reading("b/a.txt", "From the reader", 1.0), true);
    assert_eq!(
        library::book(&h.storage, "b/a.txt")
            .unwrap()
            .title
            .as_deref(),
        Some("From the reader")
    );
    // Read from the book: its own title stays.
    let mut book = library::book(&h.storage, "b/a.txt").unwrap();
    book.indexed = true;
    book.title = Some("Its own".into());
    library::save(&h.storage, "b/a.txt", &book).unwrap();
    library::record(&env, &reading("b/a.txt", "Another", 2.0), false);
    assert_eq!(
        library::book(&h.storage, "b/a.txt")
            .unwrap()
            .title
            .as_deref(),
        Some("Its own")
    );
}

#[test]
fn records_written_by_an_older_details_version_are_read_again() {
    let storage = MemoryStorage::new().with(PATH, "12345").with(
        ".paperpaw/library/books/0c6afbf2778c9bbf.ini",
        "[book]\nsize = 5\nindexed = on\nversion = 4\ntitle = New\n",
    );
    assert!(library::book(&storage, PATH).unwrap().indexed);
    for older in ["version = 3", "version = x", ""] {
        let text = format!("[book]\nsize = 5\nindexed = on\n{older}\ntitle = Old\n");
        let storage = MemoryStorage::new()
            .with(PATH, "12345")
            .with(".paperpaw/library/books/0c6afbf2778c9bbf.ini", text);
        let book = library::book(&storage, PATH).unwrap();
        assert!(!book.indexed, "{older:?}");
        assert_eq!(book.title.as_deref(), Some("Old"));
    }
}

#[test]
fn records_with_broken_lines_keep_what_they_can() {
    let text = "[book]\nsize = 5\nnot a line\nprogress = 400\ncover = c.gray\ncover-width = 9\n\
                title = \"unclosed\ntag.1 = One\n[field]\nx.y = 1\n";
    let storage = MemoryStorage::new()
        .with(PATH, "12345")
        .with(".paperpaw/library/books/0c6afbf2778c9bbf.ini", text);
    let book = library::book(&storage, PATH).unwrap();
    assert_eq!(book.progress, Some(100));
    // A cover without its height is none.
    assert_eq!(book.cover, None);
    assert_eq!(book.tags, ["One"]);
    assert_eq!(book.field("x.y"), Some("1"));
}

#[test]
fn a_record_without_a_size_belongs_to_an_empty_file_only() {
    let storage = MemoryStorage::new().with(PATH, "").with(
        ".paperpaw/library/books/0c6afbf2778c9bbf.ini",
        "[book]\ntitle = T\n",
    );
    assert_eq!(
        library::book(&storage, PATH).unwrap().title.as_deref(),
        Some("T")
    );
    assert_eq!(library::book(&storage, "books/missing.epub"), None);
}

#[test]
fn fields_are_set_replaced_and_removed() {
    let storage = MemoryStorage::new().with(PATH, "12345");
    library::set_field(&storage, PATH, "a.rating", "4");
    library::set_field(&storage, PATH, "b.source", "Calibre = \"x\"");
    library::set_field(&storage, PATH, "a.rating", "5");
    let book = library::book(&storage, PATH).unwrap();
    assert_eq!(book.size, 5);
    assert_eq!(book.field("a.rating"), Some("5"));
    assert_eq!(book.field("b.source"), Some("Calibre = \"x\""));
    assert_eq!(book.fields.len(), 2);
    library::set_field(&storage, PATH, "a.rating", "");
    assert_eq!(
        library::book(&storage, PATH).unwrap().field("a.rating"),
        None
    );
}

#[test]
fn finished_and_not_read_set_the_progress() {
    let storage = MemoryStorage::new().with(PATH, "12345");
    library::set_finished(&storage, PATH, true);
    assert_eq!(library::book(&storage, PATH).unwrap().progress, Some(100));
    library::set_finished(&storage, PATH, false);
    assert_eq!(library::book(&storage, PATH).unwrap().progress, None);
}

#[test]
fn moved_and_removed_books_take_their_place_in_the_lists() {
    let storage = MemoryStorage::new().with("a.epub", "1").with("b.epub", "2");
    library::set_favourite(&storage, "a.epub", true);
    library::set_favourite(&storage, "b.epub", true);
    library::set_field(&storage, "a.epub", "x.y", "1");
    library::moved(&storage, "a.epub", "c.epub");
    storage.rename("a.epub", "c.epub").unwrap();
    assert_eq!(library::favourites(&storage), ["b.epub", "c.epub"]);
    assert_eq!(
        library::book(&storage, "c.epub").unwrap().field("x.y"),
        Some("1")
    );
    library::removed(&storage, "b.epub");
    assert_eq!(library::favourites(&storage), ["c.epub"]);
    // Moving or removing what is not known changes nothing.
    library::moved(&storage, "none.epub", "other.epub");
    library::removed(&storage, "none.epub");
    assert_eq!(library::favourites(&storage), ["c.epub"]);
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
fn shown_titles_fall_back_to_readable_file_names() {
    let untitled = Book::default();
    assert_eq!(
        untitled.shown_title("b/alice-in_wonderland.txt"),
        "Alice In Wonderland"
    );
    assert_eq!(untitled.shown_title("b/My Book.txt"), "My Book");
    assert_eq!(untitled.shown_title("b/no-extension"), "No Extension");
    assert_eq!(untitled.shown_title("b/2024-01-02.txt"), "2024-01-02");
    assert_eq!(untitled.shown_title("b/--a--.txt"), "A");
    assert_eq!(untitled.shown_title("b/über-straße.txt"), "Über Straße");
    let empty = Book::default().title(String::new());
    assert_eq!(empty.shown_title("b/x.txt"), "x");
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

#[test]
fn books_are_found_in_folders_but_not_hidden_ones() {
    let storage = MemoryStorage::new()
        .with("a.txt", "")
        .with("b/c.txt", "")
        .with("b/d/e.txt", "")
        .with("b/.hidden/f.txt", "")
        .with(".paperpaw/g.txt", "")
        .with("b/picture.jpg", "");
    let formats = txt_formats();
    let mut found = Vec::new();
    library::books_in(&storage, &formats, "/b/", &mut |p| {
        found.push(p.to_string())
    });
    found.sort();
    assert_eq!(found, ["b/c.txt", "b/d/e.txt"]);
    let mut one = Vec::new();
    library::books_in(&storage, &formats, "b/c.txt", &mut |p| {
        one.push(p.to_string())
    });
    assert_eq!(one, ["b/c.txt"]);
    let keys = library::book_keys(&storage, &formats);
    assert_eq!(keys.len(), 3);
    assert!(keys.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn catalog_searches_every_word_without_regard_to_case() {
    let storage = MemoryStorage::new();
    let book = |title: &str, author: &str, tags: &[&str]| {
        Book::default()
            .title(title.to_string())
            .author(author.to_string())
            .tags(tags.iter().map(|t| t.to_string()).collect())
    };
    catalog::write(
        &storage,
        &[
            catalog::Record::of("b/1.txt", &book("Zebra Tales", "Ann Smith", &["Kids"])),
            catalog::Record::of("b/2.txt", &book("Apple Pie", "Bob", &["Food", "Kids"])),
            catalog::Record::of("b/3.txt", &book("Cars", "Ann Lee", &[])),
        ],
    )
    .unwrap();
    let titles = |q: &str| -> Vec<String> {
        catalog::search(&storage, q)
            .into_iter()
            .map(|r| r.title)
            .collect()
    };
    assert_eq!(titles("KIDS"), ["Apple Pie", "Zebra Tales"]);
    assert_eq!(titles("ann kids"), ["Zebra Tales"]);
    assert_eq!(titles("3.txt"), ["Cars"]);
    assert!(titles("   ").is_empty());
    assert!(titles("nothing").is_empty());
    // A catalog with broken lines is read as far as it goes.
    let storage = MemoryStorage::new().with(
        ".paperpaw/library/catalog.txt",
        b"\n\tno path\nb/x.txt\nb/y.txt\tA\tT\tS\t1\tt\textra\n\xff\xfe\tbad\n".to_vec(),
    );
    let mut read = Vec::new();
    catalog::each(&storage, &mut |r| read.push(r.path));
    assert_eq!(read, ["b/x.txt", "b/y.txt", "\u{fffd}\u{fffd}"]);
}

/// Folders whose names end in multibyte characters (the last four bytes
/// splitting a character) are listed without panics.
#[test]
fn folders_with_non_latin_names_are_listed() {
    let storage = MemoryStorage::new()
        .with("books/日本語/a.txt", "")
        .with("books/€abc/b.txt", "");
    let formats = txt_formats();
    let listing = Listing::new(&storage, &formats, "books", false, None);
    assert_eq!(listing.count(), 2);
}

/// An empty folder has an empty first page, without panics.
#[test]
fn an_empty_folder_has_an_empty_first_page() {
    let storage = MemoryStorage::new().with("books/cover.jpg", "");
    let formats = txt_formats();
    let listing = Listing::new(&storage, &formats, "books", false, None);
    assert_eq!(listing.count(), 0);
    assert!(listing.page(0, 4, 0).is_empty());
}

/// The page of an entry with no rows is empty, without dividing by
/// zero.
#[test]
fn the_page_of_an_entry_with_no_rows_does_not_panic() {
    let storage = MemoryStorage::new().with("books/a.txt", "");
    let formats = txt_formats();
    let listing = Listing::new(&storage, &formats, "books", false, None);
    let entry = Entry::new("a.txt", false, UNRANKED, 0);
    let _ = listing.page_of(&entry, 0);
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

/// The page of an entry that comes first on its page (its index a
/// multiple of the rows) is the folder's first page instead: going up
/// from a folder shown at the top of a later page shows the first page.
#[test]
fn the_page_of_an_entry_first_on_its_page_holds_it() {
    let mut storage = MemoryStorage::new();
    for i in 0..9 {
        storage = storage.with(&format!("books/{i}.txt"), "");
    }
    let formats = txt_formats();
    let listing = Listing::new(&storage, &formats, "books", false, None);
    let entry = Entry::new("4.txt", false, UNRANKED, 0);
    let (first, page) = listing.page_of(&entry, 4);
    assert_eq!(first, 4);
    let names: Vec<&str> = page.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["4.txt", "5.txt", "6.txt", "7.txt"]);
}
