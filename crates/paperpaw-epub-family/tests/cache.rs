//! The conversion cache (ADR 0011): a book opened again reads what was
//! converted; a book changed is converted anew; a damaged cache is read
//! without panics or endless loops; positions outside the book find
//! nothing rather than failing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Recording, blocks, book, book_with, text, zip};
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

const PATH: &str = "book.epub";

fn chapters() -> Vec<String> {
    (0..3)
        .map(|c| {
            (0..40)
                .map(|p| format!("<p>Chapter {c} paragraph {p} with <b>some</b> words.</p>"))
                .collect()
        })
        .collect()
}

/// The files below `dir`, recursively.
fn files(storage: &dyn Storage, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut entries = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        entries.push((entry.name.to_string(), entry.is_dir));
    });
    for (name, is_dir) in entries {
        let path = format!("{dir}/{name}");
        if is_dir {
            out.extend(files(storage, &path));
        } else {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn opening_again_converts_nothing() {
    let storage = Recording::new(MemoryStorage::new().with(PATH, book(&chapters(), &[])));
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    let first = blocks(&doc, &storage);
    storage.written.borrow_mut().clear();
    let again = EpubDocument::open(&storage, PATH).unwrap();
    assert_eq!(blocks(&again, &storage), first);
    assert_eq!(*storage.written.borrow(), Vec::<String>::new());
}

#[test]
fn a_book_changed_in_size_is_converted_anew() {
    let storage = MemoryStorage::new().with(PATH, book(&["<p>Old text</p>".into()], &[]));
    assert_eq!(
        text(&EpubDocument::open(&storage, PATH).unwrap(), &storage),
        "Old text"
    );
    storage
        .write(PATH, &book(&["<p>Brand new text</p>".into()], &[]))
        .unwrap();
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    assert_eq!(text(&doc, &storage), "Brand new text");
}

#[test]
fn a_book_changed_but_not_in_size_is_converted_anew() {
    // Stored, so equal text lengths give equal file sizes; a typo fixed
    // in a book's new edition does the same.
    let old = book_with(&["<p>Alpha text</p>".into()], "", &[], true);
    let new = book_with(&["<p>Omega text</p>".into()], "", &[], true);
    assert_eq!(old.len(), new.len());
    let storage = MemoryStorage::new().with(PATH, old);
    assert_eq!(
        text(&EpubDocument::open(&storage, PATH).unwrap(), &storage),
        "Alpha text"
    );
    storage.write(PATH, &new).unwrap();
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    assert_eq!(text(&doc, &storage), "Omega text");
}

#[test]
fn a_chapter_converted_halfway_is_converted_again() {
    // The end marker is written last: without it, a conversion broken off
    // (power lost) is done again.
    let storage = MemoryStorage::new().with(PATH, book(&chapters(), &[]));
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    let first = blocks(&doc, &storage);
    let cache = files(&storage, ".paperpaw/plugins/format-epub");
    let end = cache
        .iter()
        .find(|p| p.ends_with("/sections/1/end"))
        .unwrap();
    let text_file = end.replace("/end", "/txt");
    storage.remove(end).unwrap();
    let mut half = storage.get(&text_file).unwrap();
    half.truncate(half.len() / 2);
    storage.write(&text_file, &half).unwrap();
    let again = EpubDocument::open(&storage, PATH).unwrap();
    assert_eq!(blocks(&again, &storage), first);
}

#[test]
fn positions_outside_the_book_find_nothing() {
    let storage = MemoryStorage::new().with(PATH, book(&chapters(), &[]));
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    let sections = doc.sections();
    for at in [
        Locator::new(sections, 0),
        Locator::new(u32::MAX, 0),
        Locator::new(0, u32::MAX),
    ] {
        assert_eq!(doc.block_at(&storage, at).unwrap(), None, "{at:?}");
        assert!(doc.progress(at) <= 1.0);
        assert_eq!(doc.page_label(&storage, at), None);
    }
    assert_eq!(doc.links(&storage, 0, u32::MAX, u32::MAX), Vec::new());
    assert_eq!(
        doc.locate_xpointer(&storage, "/body/DocFragment[99]/body/p"),
        None
    );
    assert_eq!(
        doc.locate_xpointer(&storage, "/body/DocFragment[0]/body/p"),
        None
    );
    assert_eq!(
        doc.locate_xpointer(&storage, "/body/DocFragment[99999999999]/body/p"),
        None
    );
}

#[test]
fn links_and_xpointers_of_sections_past_the_end_find_nothing() {
    // A position kept from an earlier edition of the book with more
    // chapters, or a caller's slip: `chapter()` indexes the spine.
    let storage = MemoryStorage::new().with(PATH, book(&chapters(), &[]));
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    let past = doc.sections();
    let links = catch_unwind(AssertUnwindSafe(|| doc.links(&storage, past, 0, u32::MAX)));
    assert_eq!(links.ok(), Some(Vec::new()));
    let xpointer = catch_unwind(AssertUnwindSafe(|| {
        doc.xpointer(&storage, Locator::new(past, 0))
    }));
    assert_eq!(xpointer.ok(), Some(None));
}

/// Positions are those of one conversion: the one with the book's own
/// styles stores style runs the one without them lacks (and shows text
/// they hide), so the same text is at other offsets. A position goes
/// across as an XPointer, which does not depend on the conversion; the
/// reader keeps one with the position (reader
/// `tests/navigation.rs::the_place_stays_on_its_text_when_the_books_styles_switch`).
#[test]
fn positions_go_across_the_books_styles_as_xpointers() {
    let body: String = (0..30)
        .map(|i| format!("<p class=\"bold\">Paragraph number {i}.</p>"))
        .collect();
    let epub = book_with(&[body], ".bold { font-weight: bold }", &[], false);
    let storage = MemoryStorage::new().with(PATH, epub);
    let styled = EpubDocument::open_with(&storage, PATH, true).unwrap();
    let plain = EpubDocument::open_with(&storage, PATH, false).unwrap();
    for (_, block) in blocks(&styled, &storage) {
        let xpointer = styled
            .xpointer(&storage, Locator::new(0, block.start))
            .unwrap();
        let at = plain.locate_xpointer(&storage, &xpointer).unwrap();
        let other = plain.block_at(&storage, at).unwrap().unwrap();
        assert_eq!(other.text, block.text, "{xpointer}");
    }
}

#[test]
fn a_damaged_cache_is_read_without_panic_or_endless_loop() {
    // Every cache file of a converted book, cut short or emptied.
    let epub = book(&chapters(), &[]);
    let base = MemoryStorage::new().with(PATH, epub.clone());
    blocks(&EpubDocument::open(&base, PATH).unwrap(), &base);
    let cache = files(&base, ".paperpaw/plugins/format-epub");
    assert!(cache.len() > 10, "{cache:?}");
    for file in &cache {
        for keep in [0usize, 3, 17] {
            let storage = MemoryStorage::new().with(PATH, epub.clone());
            for path in &cache {
                let mut data = base.get(path).unwrap();
                if path == file {
                    data.truncate(keep.min(data.len()));
                }
                storage.write(path, &data).unwrap();
            }
            read_bounded(&storage);
        }
    }
}

/// Opens the book and reads it as a reader would, within bounds.
fn read_bounded(storage: &MemoryStorage) {
    let Ok(doc) = EpubDocument::open(storage, PATH) else {
        return;
    };
    for section in 0..doc.sections().min(8) {
        let mut at = Locator::new(section, 0);
        for _ in 0..10_000 {
            match doc.block_at(storage, at) {
                Ok(Some(block)) => {
                    assert!(block.end > at.offset, "no progress at {at:?}");
                    let _ = doc.xpointer(storage, at);
                    at = Locator::new(section, block.end);
                }
                _ => break,
            }
        }
    }
    for i in 0..doc.toc_len(storage).min(20) {
        let _ = (doc.toc_entry(storage, i), doc.toc_target(storage, i));
    }
    let _ = doc.fonts(storage);
}

proptest! {
    #![proptest_config(common::cases(64))]

    #[test]
    fn damaged_cache_files_never_panic(
        file in any::<prop::sample::Index>(),
        damage in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
    ) {
        let epub = book(&chapters()[..2], &[]);
        let storage = MemoryStorage::new().with(PATH, epub);
        blocks(&EpubDocument::open(&storage, PATH).unwrap(), &storage);
        let cache = files(&storage, ".paperpaw/plugins/format-epub");
        let path = file.get(&cache);
        let mut data = storage.get(path).unwrap();
        if !data.is_empty() {
            for (at, byte) in damage {
                let at = at.index(data.len());
                data[at] = byte;
            }
        }
        storage.write(path, &data).unwrap();
        // Contents entries damaged to lead past the last chapter panic
        // (see `contents_damaged_to_lead_past_the_last_chapter_find_nothing`).
        if let Ok(doc) = EpubDocument::open(&storage, PATH) {
            let sections = doc.sections();
            prop_assume!((0..doc.toc_len(&storage).min(20)).all(|i| {
                doc.toc_entry(&storage, i).is_none_or(|e| e.at.section < sections)
            }));
        }
        read_bounded(&storage);
    }
}

/// The shrunk case of `damaged_cache_files_never_panic`: two bytes of
/// `toc.dat` changed, its first entry now leading to a third chapter of
/// two, at an anchor.
#[test]
fn contents_damaged_to_lead_past_the_last_chapter_find_nothing() {
    let epub = book(&chapters()[..2], &[]);
    let storage = MemoryStorage::new().with(PATH, epub);
    blocks(&EpubDocument::open(&storage, PATH).unwrap(), &storage);
    let cache = files(&storage, ".paperpaw/plugins/format-epub");
    let toc = cache.iter().find(|p| p.ends_with("/toc.dat")).unwrap();
    let mut data = storage.get(toc).unwrap();
    // Record: level, section (u32), fragment length (u16), title length
    // (u16), fragment, title.
    data[1] = 2;
    data[5] = 1;
    storage.write(toc, &data).unwrap();
    let doc = EpubDocument::open(&storage, PATH).unwrap();
    assert_eq!(doc.toc_entry(&storage, 0).unwrap().at.section, 2);
    let target = catch_unwind(AssertUnwindSafe(|| doc.toc_target(&storage, 0)));
    assert!(target.is_ok(), "toc_target panicked");
}

#[test]
fn caches_of_different_books_are_kept_apart() {
    let storage = MemoryStorage::new()
        .with("a/book.epub", book(&["<p>First book</p>".into()], &[]))
        .with(
            "b/book.epub",
            zip(
                &[
                    (
                        "META-INF/container.xml",
                        common::container("x.opf").into_bytes(),
                    ),
                    (
                        "x.opf",
                        common::opf("", &[("c", "c.xhtml", "application/xhtml+xml", "")], &["c"])
                            .into_bytes(),
                    ),
                    ("c.xhtml", common::xhtml("<p>Second book</p>").into_bytes()),
                ],
                false,
            ),
        );
    let a = EpubDocument::open(&storage, "a/book.epub").unwrap();
    let b = EpubDocument::open(&storage, "b/book.epub").unwrap();
    assert_eq!(text(&a, &storage), "First book");
    assert_eq!(text(&b, &storage), "Second book");
    assert_eq!(text(&a, &storage), "First book");
}
