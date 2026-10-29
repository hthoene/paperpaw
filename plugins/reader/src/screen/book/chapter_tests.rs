//! Skimming by chapters finds what all exact starts find, looking at few
//! of them closer.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::document::{Block, Document, DocumentError, Locator, Metadata, TocEntry};
use paperpaw_sdk::{MemoryStorage, Storage};

use super::*;

/// Entries by the locator they say and their exact start, if any:
/// `None` as the first for an entry that says none.
struct Toc {
    entries: Vec<(Option<Locator>, Option<Locator>)>,
    metadata: Metadata,
    exact: Cell<usize>,
}

impl Document for Toc {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        5
    }

    fn section_len(&self, _: u32) -> u32 {
        100
    }

    fn block_at(&self, _: &dyn Storage, _: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(None)
    }

    fn toc_len(&self, _: &dyn Storage) -> u32 {
        self.entries.len() as u32
    }

    fn toc_entry(&self, _: &dyn Storage, index: u32) -> Option<TocEntry> {
        let at = self.entries.get(index as usize)?.0?;
        Some(TocEntry {
            title: String::new(),
            level: 0,
            at,
        })
    }

    fn toc_target(&self, _: &dyn Storage, index: u32) -> Option<Locator> {
        self.exact.set(self.exact.get() + 1);
        self.entries.get(index as usize)?.1
    }
}

/// A section and offset, if any.
type At = Option<(u32, u32)>;

fn toc(entries: &[(At, At)]) -> Toc {
    let at = |l: Option<(u32, u32)>| l.map(|(s, o)| Locator::new(s, o));
    Toc {
        entries: entries.iter().map(|&(a, b)| (at(a), at(b))).collect(),
        metadata: Metadata::default(),
        exact: Cell::new(0),
    }
}

/// Every page from `(s, o)` to `(s, o + 10)` and across a section's end.
fn pages() -> impl Iterator<Item = (Locator, Locator)> {
    (0..5u32).flat_map(|s| {
        (0..100u32).step_by(5).flat_map(move |o| {
            [
                (Locator::new(s, o), Locator::new(s, o + 10)),
                (Locator::new(s, o), Locator::new(s + 1, 0)),
            ]
        })
    })
}

fn same_as_all_starts(toc: &Toc) {
    let storage = MemoryStorage::new();
    let n = toc.toc_len(&storage);
    for (here, end) in pages() {
        assert_eq!(
            next_chapter(toc, &storage, n, end),
            chapter_by_starts(toc, &storage, n, (here, end), true),
            "on from {end:?}"
        );
        assert_eq!(
            previous_chapter(toc, &storage, n, here),
            chapter_by_starts(toc, &storage, n, (here, end), false),
            "back from {here:?}"
        );
    }
}

#[test]
fn in_reading_order_with_anchors() {
    let book = toc(&[
        (Some((0, 0)), Some((0, 0))),
        (Some((1, 0)), Some((1, 0))),
        (Some((1, 0)), Some((1, 40))),
        (Some((1, 0)), Some((1, 70))),
        (Some((2, 0)), Some((2, 30))),
        (Some((2, 0)), Some((2, 30))),
        (Some((3, 0)), Some((3, 0))),
        (Some((4, 0)), Some((4, 55))),
    ]);
    same_as_all_starts(&book);
}

#[test]
fn out_of_order_unknown_and_missing_starts() {
    let book = toc(&[
        (Some((3, 0)), Some((3, 20))),
        (Some((1, 0)), Some((1, 50))),
        (None, Some((2, 10))),
        (Some((2, 0)), None),
        (Some((4, 0)), Some((4, 0))),
        (Some((0, 0)), Some((0, 60))),
        (Some((1, 30)), Some((1, 30))),
        (None, None),
        (Some((3, 0)), None),
    ]);
    same_as_all_starts(&book);
    // Starts only before the page, none of the section before it.
    same_as_all_starts(&toc(&[
        (Some((0, 0)), Some((0, 20))),
        (Some((2, 0)), None),
        (Some((2, 0)), None),
    ]));
    same_as_all_starts(&toc(&[]));
}

#[test]
fn few_exact_starts_are_found() {
    // A chapter a section, each but the first at an anchor.
    let entries: Vec<_> = (0..5u32)
        .map(|s| (Some((s, 0)), Some((s, if s == 0 { 0 } else { 5 }))))
        .collect();
    let book = toc(&entries);
    let storage = MemoryStorage::new();
    assert_eq!(
        next_chapter(&book, &storage, 5, Locator::new(2, 40)),
        Some(Locator::new(3, 5))
    );
    assert_eq!(book.exact.get(), 2);
    book.exact.set(0);
    assert_eq!(
        previous_chapter(&book, &storage, 5, Locator::new(3, 5)),
        Some(Locator::new(2, 5))
    );
    assert_eq!(book.exact.get(), 2);
}

/// The index of pages as the layout makes them: a page that begins a
/// section starts at the section's first block, after offset 0.
fn index_of(storage: &MemoryStorage, starts: &[Locator]) -> PageIndex {
    let mut index = PageIndex::open(storage, String::from("pages.bin"));
    assert!(index.append(storage, starts));
    index.finish(storage);
    index
}

#[test]
fn a_place_before_a_sections_first_page_is_on_that_page() {
    let storage = MemoryStorage::new();
    let index = index_of(
        &storage,
        &[
            Locator::new(0, 8),
            Locator::new(0, 100),
            Locator::new(1, 8),
            Locator::new(1, 100),
        ],
    );
    let page = |section, offset| indexed_page(&index, &storage, Locator::new(section, offset));
    // The start of a chapter that begins a file: its first page, not the
    // last of the section before.
    assert_eq!(page(1, 0), Some((2, Locator::new(1, 8))));
    // Inside a page, and at its start: that page.
    assert_eq!(page(1, 8), Some((2, Locator::new(1, 8))));
    assert_eq!(page(1, 50), Some((2, Locator::new(1, 8))));
    assert_eq!(page(1, 100), Some((3, Locator::new(1, 100))));
    assert_eq!(page(0, 99), Some((0, Locator::new(0, 8))));
    // Before the first page of the book (a cover, a first chapter).
    assert_eq!(page(0, 0), Some((0, Locator::new(0, 8))));
    // Past the last page: the last page, as `locate` has it.
    assert_eq!(page(2, 0), Some((3, Locator::new(1, 100))));
}

#[test]
fn a_section_without_pages_is_left_to_the_page_before() {
    let storage = MemoryStorage::new();
    let index = index_of(&storage, &[Locator::new(0, 8), Locator::new(2, 8)]);
    assert_eq!(
        indexed_page(&index, &storage, Locator::new(1, 0)),
        Some((0, Locator::new(0, 8)))
    );
}

#[test]
fn no_pages_no_page() {
    let storage = MemoryStorage::new();
    let index = PageIndex::open(&storage, String::from("pages.bin"));
    assert_eq!(indexed_page(&index, &storage, Locator::new(0, 0)), None);
}
