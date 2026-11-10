//! Reading positions (ADR 0007): a position taken on a page is found
//! again after the book is laid out anew with another font size, line
//! width and page height, and after it is opened again (from the cache,
//! or converted afresh); KOReader's XPointers of it lead back to it.
//! Property-based over generated books, layouts and positions.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::book_with;
use paperpaw_layout::{Face, LayoutContext, Page, Style, Typesetter, layout_page, line_start};
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, Locator};
use proptest::prelude::*;

/// Every character equally wide; bold and headings wider, headings
/// taller.
#[derive(Debug, Clone, Copy)]
struct Mono {
    advance: f32,
    line: f32,
}

impl Typesetter for Mono {
    fn advance(&self, face: Face, _: char) -> f32 {
        if face.heading > 0 || face.style.bold() {
            self.advance * 1.25
        } else {
            self.advance
        }
    }

    fn line(&self, face: Face) -> (f32, f32) {
        let height = if face.heading > 0 {
            self.line * 1.5
        } else {
            self.line
        };
        (height * 0.8, height)
    }
}

/// A layout: the type, the line width and the page height.
#[derive(Debug, Clone, Copy)]
struct Layout {
    mono: Mono,
    width: f32,
    height: f32,
}

const STYLE: Style = Style::new(4.0, 10.0, 12.0, true);

fn context<'a>(
    doc: &'a dyn Document,
    storage: &'a MemoryStorage,
    layout: &'a Layout,
) -> LayoutContext<'a> {
    LayoutContext::new(doc, storage, layout.width, &STYLE, &layout.mono)
}

/// All pages of `doc` laid out as `layout` says.
fn paginate(doc: &dyn Document, storage: &MemoryStorage, layout: &Layout) -> Vec<Page> {
    let cx = context(doc, storage, layout);
    let mut pages = Vec::new();
    let mut at = Locator::START;
    loop {
        let page = layout_page(&cx, at, layout.height).unwrap();
        if page.is_past_end() {
            return pages;
        }
        assert!(page.end > at, "no progress at {at:?}");
        at = page.end;
        pages.push(page);
        assert!(pages.len() < 100_000);
    }
}

/// Whether `page` shows a word at `at`.
fn shows(page: &Page, at: Locator) -> bool {
    page.lines.iter().any(|line| {
        line.section == at.section
            && line
                .words
                .iter()
                .any(|&(s, e)| s <= at.offset && at.offset < e)
    })
}

/// The page of `pages` that `at` falls in: the last starting at or
/// before it.
fn page_of(pages: &[Page], at: Locator) -> &Page {
    let i = pages.partition_point(|p| p.start <= at);
    &pages[i.saturating_sub(1)]
}

fn layout() -> impl Strategy<Value = Layout> {
    (
        4.0f32..14.0,
        10.0f32..30.0,
        120.0f32..700.0,
        150.0f32..900.0,
    )
        .prop_map(|(advance, line, width, height)| Layout {
            mono: Mono { advance, line },
            width,
            height: height.max(line * 4.0),
        })
}

const WORDS: &[&str] = &[
    "the",
    "a",
    "reading",
    "position",
    "Über",
    "naïve",
    "e-reader",
    "longerwordthanmost",
    "€5",
    "x",
    "and",
    "pages",
    "it's",
    "\u{2014}",
    "Kafka",
];

/// Chapters of paragraphs: headings, plain and bold words, now and then
/// a paragraph longer than a block holds.
fn chapters() -> impl Strategy<Value = Vec<String>> {
    let para = (
        0usize..5,
        prop_oneof![
            6 => prop::collection::vec(prop::sample::select(WORDS), 1..60),
            1 => prop::collection::vec(prop::sample::select(WORDS), 900..1100),
        ],
    )
        .prop_map(|(kind, words)| {
            let text: Vec<String> = words
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    if i % 9 == 4 {
                        format!("<b>{w}</b>")
                    } else {
                        (*w).to_string()
                    }
                })
                .collect();
            let text = text.join(" ");
            match kind {
                0 => format!("<h2>{text}</h2>"),
                1 => format!("<div><p class=\"c\">{text}</p></div>"),
                _ => format!("<p>{text}</p>"),
            }
        });
    prop::collection::vec(
        prop::collection::vec(para, 1..10).prop_map(|p| p.concat()),
        1..4,
    )
}

proptest! {
    #![proptest_config(common::cases(32))]

    #[test]
    fn positions_survive_relayout(
        bodies in chapters(),
        a in layout(),
        b in layout(),
        picks in prop::collection::vec((any::<prop::sample::Index>(), any::<prop::sample::Index>()), 1..12),
    ) {
        let epub = book_with(&bodies, ".c { text-align: center }", &[], false);
        let storage = MemoryStorage::new().with("book.epub", epub);
        let doc = EpubDocument::open(&storage, "book.epub").unwrap();
        let first = paginate(&doc, &storage, &a);
        let second = paginate(&doc, &storage, &b);
        prop_assume!(!first.is_empty());
        let cx = context(&doc, &storage, &b);
        let mut checked = 0;
        for (page, word) in picks {
            // A position on a page of the first layout: its start, or a
            // word on it.
            let page = page.get(&first);
            let words: Vec<Locator> = page
                .lines
                .iter()
                .flat_map(|l| l.words.iter().map(move |&(s, _)| Locator::new(l.section, s)))
                .collect();
            for at in [page.start, *word.get(&words)] {
                if !shows(page, at) {
                    // A page that starts in the space before a block.
                    continue;
                }
                let found = page_of(&second, at);
                prop_assert!(shows(found, at), "{at:?} not on {:?}..{:?}", found.start, found.end);
                // Shown from that position, a page starts with its line.
                let from = line_start(&cx, at).unwrap();
                let shown = layout_page(&cx, from, b.height).unwrap();
                prop_assert!(shows(&shown, at), "{at:?} not on the page from {from:?}");
                prop_assert!(
                    shown.lines.first().is_some_and(|l| l.words.iter().any(|&(s, e)| s <= at.offset && at.offset < e)),
                    "{at:?} not on the first line from {from:?}"
                );
                checked += 1;
            }
        }
        prop_assert!(checked > 0);
    }

    #[test]
    fn positions_survive_opening_again_and_converting_afresh(
        bodies in chapters(),
        picks in prop::collection::vec((any::<prop::sample::Index>(), any::<prop::sample::Index>()), 1..16),
    ) {
        let epub = book_with(&bodies, "", &[], false);
        let storage = MemoryStorage::new().with("book.epub", epub.clone());
        let doc = EpubDocument::open(&storage, "book.epub").unwrap();
        let again = EpubDocument::open(&storage, "book.epub").unwrap();
        let other = MemoryStorage::new().with("book.epub", epub);
        let fresh = EpubDocument::open(&other, "book.epub").unwrap();
        for (section, offset) in picks {
            let section = section.index(doc.sections() as usize) as u32;
            let len = doc.section_len(section).max(1);
            let at = Locator::new(section, offset.index(len as usize) as u32);
            let block = doc.block_at(&storage, at).unwrap();
            prop_assert_eq!(&again.block_at(&storage, at).unwrap(), &block);
            prop_assert_eq!(&fresh.block_at(&other, at).unwrap(), &block);
            prop_assert_eq!(again.xpointer(&storage, at), doc.xpointer(&storage, at));
            prop_assert_eq!(fresh.xpointer(&other, at), doc.xpointer(&storage, at));
        }
    }

    #[test]
    fn xpointers_of_words_lead_back_to_them(
        bodies in chapters(),
        picks in prop::collection::vec((any::<prop::sample::Index>(), any::<prop::sample::Index>()), 1..16),
    ) {
        let epub = book_with(&bodies, "", &[], false);
        let storage = MemoryStorage::new().with("book.epub", epub);
        let doc = EpubDocument::open(&storage, "book.epub").unwrap();
        let blocks = common::blocks(&doc, &storage);
        for (block, word) in picks {
            let (section, block) = block.get(&blocks);
            // The start of a word of the block.
            let starts: Vec<usize> = block
                .text
                .char_indices()
                .filter(|&(i, c)| !c.is_whitespace() && (i == 0 || block.text[..i].ends_with(char::is_whitespace)))
                .map(|(i, _)| i)
                .collect();
            let index = *word.get(&starts);
            let at = Locator::new(*section, block.source_offset(index));
            let xpointer = doc.xpointer(&storage, at).unwrap();
            prop_assert!(xpointer.contains("text()"), "{xpointer}");
            prop_assert_eq!(doc.locate_xpointer(&storage, &xpointer), Some(at), "{}", xpointer);
        }
    }
}
