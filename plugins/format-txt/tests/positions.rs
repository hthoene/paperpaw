//! Reading positions in plain text (ADR 0007: byte offsets into the
//! file), over generated texts: every position inside a block finds that
//! block wherever reading started; all text comes through, its offsets
//! pointing back into the file; a position taken on a page is found again
//! after the text is laid out anew with other type, width and height.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_layout::{Face, LayoutContext, Page, Style, Typesetter, layout_page, line_start};
use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Block, Document, Locator};
use proptest::prelude::*;

const WORDS: &[&str] = &[
    "the",
    "a",
    "Größe",
    "naïve",
    "CHAPTER",
    "I.",
    "€",
    "word",
    "longerwordthanmostwordsare",
    "it's",
    "\u{2014}",
    "x",
];

/// A text: paragraphs of words, wrapped into lines or not, separated by
/// blank lines or not, with Unix or Windows line ends, and now and then a
/// paragraph longer than a block holds.
fn text() -> impl Strategy<Value = String> {
    text_of(1)
}

/// As [`text`]; long paragraphs `long` in nine (0: none).
fn text_of(long: u32) -> impl Strategy<Value = String> {
    let para = prop_oneof![
        9 - long => prop::collection::vec(prop::sample::select(WORDS), 1..40),
        long => prop::collection::vec(prop::sample::select(WORDS), 1500..2500),
    ];
    (
        prop::collection::vec(para, 1..20),
        any::<bool>(),
        any::<bool>(),
        prop::sample::select(&[0usize, 8, 70][..]),
    )
        .prop_map(|(paras, blank, crlf, wrap)| {
            let end = if crlf { "\r\n" } else { "\n" };
            let mut out = String::new();
            for para in paras {
                let mut line = 0;
                for (i, word) in para.iter().enumerate() {
                    if i > 0 {
                        if wrap > 0 && line + word.len() > wrap {
                            out.push_str(end);
                            line = 0;
                        } else {
                            out.push(' ');
                            line += 1;
                        }
                    }
                    out.push_str(word);
                    line += word.len();
                }
                out.push_str(end);
                if blank {
                    out.push_str(end);
                }
            }
            out
        })
}

fn open(text: &str) -> (MemoryStorage, TxtDocument) {
    let storage = MemoryStorage::new().with("t.txt", text.as_bytes().to_vec());
    let doc = TxtDocument::open_converted(&storage, "t.txt", "").unwrap();
    (storage, doc)
}

fn blocks(doc: &TxtDocument, storage: &MemoryStorage) -> Vec<Block> {
    let mut at = Locator::START;
    let mut out = Vec::new();
    while let Some(block) = doc.block_at(storage, at).unwrap() {
        assert!(block.end > at.offset, "no progress at {at:?}");
        at = Locator::new(0, block.end);
        out.push(block);
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Mono {
    advance: f32,
    line: f32,
}

impl Typesetter for Mono {
    fn advance(&self, face: Face, _: char) -> f32 {
        if face.heading > 0 {
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

#[derive(Debug, Clone, Copy)]
struct Layout {
    mono: Mono,
    width: f32,
    height: f32,
}

const STYLE: Style = Style::new(4.0, 10.0, 12.0, false);

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

fn context<'a>(
    doc: &'a TxtDocument,
    storage: &'a MemoryStorage,
    layout: &'a Layout,
) -> LayoutContext<'a> {
    LayoutContext::new(doc, storage, layout.width, &STYLE, &layout.mono)
}

fn paginate(doc: &TxtDocument, storage: &MemoryStorage, layout: &Layout) -> Vec<Page> {
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
    }
}

fn shows(page: &Page, at: Locator) -> bool {
    page.lines.iter().any(|line| {
        line.words
            .iter()
            .any(|&(s, e)| s <= at.offset && at.offset < e)
    })
}

/// `cases` cases, or as many as `PROPTEST_CASES` says, for longer runs.
fn cases(cases: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(cases);
    ProptestConfig::with_cases(cases)
}

proptest! {
    #![proptest_config(cases(48))]

    #[test]
    fn every_position_in_a_block_finds_that_block(
        // Long paragraphs are split differently from different positions
        // (see `a_long_first_paragraph_is_one_block_from_any_position`).
        text in text_of(0),
        picks in prop::collection::vec(any::<prop::sample::Index>(), 1..24),
    ) {
        let (storage, doc) = open(&text);
        let all = blocks(&doc, &storage);
        prop_assume!(!all.is_empty());
        for pick in picks {
            let block = pick.get(&all);
            // Up to its last character: the white space after a paragraph
            // leads to the next one.
            let last = match block.text.char_indices().last() {
                Some((i, _)) => block.source_offset(i),
                None => block.start,
            };
            for offset in [block.start, block.start + (last - block.start) / 2, last] {
                let found = doc.block_at(&storage, Locator::new(0, offset)).unwrap().unwrap();
                prop_assert_eq!(&found, block, "at {}", offset);
            }
        }
        // Any offset at all finds the block it is in, or the next one.
        for pick in 0..text.len() as u32 / 97 {
            let offset = pick * 97;
            if let Some(found) = doc.block_at(&storage, Locator::new(0, offset)).unwrap() {
                prop_assert!(all.contains(&found));
                prop_assert!(found.end > offset);
            }
        }
    }

    #[test]
    fn all_text_comes_through_and_points_back(text in text()) {
        let (storage, doc) = open(&text);
        let all = blocks(&doc, &storage);
        let shown: Vec<&str> = all.iter().flat_map(|b| b.text.split_whitespace()).collect();
        let expected: Vec<&str> = text.split_whitespace().collect();
        // Words of a paragraph longer than a block holds may be cut where
        // no space is near: compare without spaces then.
        prop_assert_eq!(shown.concat(), expected.concat());
        for block in &all {
            prop_assert!(block.text.len() <= 4096 + 256);
            for (i, c) in block.text.char_indices().filter(|(_, c)| !c.is_whitespace()) {
                let at = block.source_offset(i) as usize;
                prop_assert!(text[at..].starts_with(c), "{:?} at {}", c, at);
                prop_assert_eq!(block.text_index(at as u32), i);
            }
        }
    }

    #[test]
    fn positions_survive_relayout(
        text in text(),
        a in layout(),
        b in layout(),
        picks in prop::collection::vec((any::<prop::sample::Index>(), any::<prop::sample::Index>()), 1..12),
    ) {
        let (storage, doc) = open(&text);
        let first = paginate(&doc, &storage, &a);
        let second = paginate(&doc, &storage, &b);
        prop_assume!(!first.is_empty());
        let cx = context(&doc, &storage, &b);
        let mut checked = 0;
        for (page, word) in picks {
            let page = page.get(&first);
            let words: Vec<Locator> = page
                .lines
                .iter()
                .flat_map(|l| l.words.iter().map(|&(s, _)| Locator::new(0, s)))
                .collect();
            if words.is_empty() {
                continue;
            }
            let at = *word.get(&words);
            let i = second.partition_point(|p| p.start <= at);
            let found = &second[i.saturating_sub(1)];
            prop_assert!(shows(found, at), "{:?} not on {:?}..{:?}", at, found.start, found.end);
            let from = line_start(&cx, at).unwrap();
            let shown = layout_page(&cx, from, b.height).unwrap();
            prop_assert!(
                shown.lines.first().is_some_and(|l| l.words.iter().any(|&(s, e)| s <= at.offset && at.offset < e)),
                "{:?} not on the first line from {:?}", at, from
            );
            checked += 1;
        }
        prop_assert!(checked > 0);
    }
}

/// The shrunk case of `every_position_in_a_block_finds_that_block` with
/// long paragraphs: a paragraph longer than a block holds, at the file's
/// start.
#[test]
fn a_long_first_paragraph_is_one_block_from_any_position() {
    // Its pieces start after the first white space past each multiple of
    // 2048 bytes; the paragraph starts at 0, so the first piece starts
    // after "the " when read from offset 4 on, but at 0 from 0 to 3.
    let text = "the ".repeat(1500);
    let (storage, doc) = open(&text);
    let from_start = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
    let from_inside = doc
        .block_at(&storage, Locator::new(0, 100))
        .unwrap()
        .unwrap();
    assert_eq!(from_inside, from_start);
}

/// The same where a paragraph starts a few bytes after a grid point,
/// not at the file's start.
#[test]
fn a_long_paragraph_starting_at_a_grid_point_is_one_block_from_any_position() {
    // A first paragraph of 2046 bytes and a blank line: the second starts
    // at 2048, its first word ending past the grid point there.
    let first = format!("{}\n\n", "ab ".repeat(682));
    assert_eq!(first.len(), 2048);
    let text = format!("{first}{}\n", "wordy ".repeat(1500));
    let (storage, doc) = open(&text);
    let start = doc
        .block_at(&storage, Locator::new(0, 2048))
        .unwrap()
        .unwrap();
    assert_eq!(start.start, 2048);
    let inside = doc
        .block_at(&storage, Locator::new(0, 2100))
        .unwrap()
        .unwrap();
    assert_eq!(inside, start);
}
