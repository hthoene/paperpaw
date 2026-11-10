//! Properties of the EPUB conversion over books generated in the test:
//! every word of every chapter comes back once, in order, whatever the
//! markup around it; conversion is deterministic and the cache gives back
//! what converting gave; arbitrary bytes and damaged books never panic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{blocks, book_with, words};
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, Locator};
use proptest::prelude::*;

/// Characters of generated words, markup characters among them.
const CHARS: &[char] = &[
    'a', 'b', 'e', 'k', 'z', 'Q', '0', '7', 'ä', 'ß', 'é', '€', '中', '&', '<', '>', '"', '\'',
    '-', '.',
];

fn word() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(CHARS), 1..10).prop_map(|c| c.into_iter().collect())
}

/// A paragraph: its element, its words and which are wrapped in inline
/// markup (0 none, else `b`, `i`, `em`, `span`, `a`).
#[derive(Debug, Clone)]
struct Para {
    element: usize,
    words: Vec<(String, usize)>,
}

fn para() -> impl Strategy<Value = Para> {
    let words = prop_oneof![
        8 => prop::collection::vec((word(), 0usize..6), 1..30),
        // Now and then one longer than a block holds.
        1 => prop::collection::vec((word(), 0usize..6), 700..900),
    ];
    (0usize..6, words).prop_map(|(element, words)| Para { element, words })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The XHTML body of a chapter of `paras`.
fn body(paras: &[Para]) -> String {
    let mut out = String::new();
    for para in paras {
        let (open, close) = [
            ("<p>", "</p>"),
            ("<h2>", "</h2>"),
            ("<div>", "</div>"),
            ("<blockquote><p>", "</p></blockquote>"),
            ("<p class=\"x\" style=\"text-align: center\">", "</p>"),
            ("<section><div><p>", "</p></div></section>"),
        ][para.element];
        out.push_str(open);
        for (i, (word, inline)) in para.words.iter().enumerate() {
            if i > 0 {
                out.push_str(if i % 7 == 0 { "\n  " } else { " " });
            }
            let word = escape(word);
            match inline {
                1 => out.push_str(&format!("<b>{word}</b>")),
                2 => out.push_str(&format!("<i>{word}</i>")),
                3 => out.push_str(&format!("<em><b>{word}</b></em>")),
                4 => out.push_str(&format!("<span class=\"s\">{word}</span>")),
                5 => out.push_str(&format!("<a href=\"#n{i}\" id=\"n{i}\">{word}</a>")),
                _ => out.push_str(&word),
            }
        }
        out.push_str(close);
        out.push('\n');
    }
    out
}

fn chapters() -> impl Strategy<Value = Vec<Vec<Para>>> {
    prop::collection::vec(prop::collection::vec(para(), 1..8), 1..4)
}

/// Opens `epub` (at `book.epub`) on `storage`.
fn open(storage: &MemoryStorage) -> EpubDocument {
    EpubDocument::open(storage, "book.epub").expect("generated books open")
}

/// Opens and reads as a reader would, within bounds; never fails.
fn exercise(data: Vec<u8>) {
    let storage = MemoryStorage::new().with("book.epub", data);
    let Ok(doc) = EpubDocument::open(&storage, "book.epub") else {
        return;
    };
    let _ = doc.metadata();
    for section in 0..doc.sections().min(4) {
        let mut at = Locator::new(section, 0);
        for _ in 0..2000 {
            match doc.block_at(&storage, at) {
                Ok(Some(block)) => {
                    assert!(block.end > at.offset, "no progress at {at:?}");
                    let _ = doc.xpointer(&storage, at);
                    at = Locator::new(section, block.end);
                }
                _ => break,
            }
        }
    }
    for i in 0..doc.toc_len(&storage).min(20) {
        let _ = doc.toc_target(&storage, i);
    }
}

proptest! {
    #![proptest_config(common::cases(48))]

    #[test]
    fn generated_books_give_back_all_their_words_in_order(
        chapters in chapters(),
        stored in any::<bool>(),
    ) {
        let bodies: Vec<String> = chapters.iter().map(|c| body(c)).collect();
        let css = ".x { font-weight: bold } .s { font-style: italic }";
        let epub = book_with(&bodies, css, &[], stored);
        let storage = MemoryStorage::new().with("book.epub", epub.clone());
        let doc = open(&storage);
        prop_assert_eq!(doc.sections() as usize, chapters.len());
        let expected: Vec<String> = chapters
            .iter()
            .flatten()
            .flat_map(|p| p.words.iter().map(|(w, _)| w.clone()))
            .collect();
        let first = blocks(&doc, &storage);
        let got: Vec<String> = first
            .iter()
            .flat_map(|(_, b)| b.text.split_whitespace().map(String::from).collect::<Vec<_>>())
            .collect();
        prop_assert_eq!(&got, &expected);
        // No block is longer than the conversion's limit.
        prop_assert!(first.iter().all(|(_, b)| b.text.len() <= 4000));
        // Each chapter's contents entry leads to its start.
        for i in 0..doc.toc_len(&storage) {
            prop_assert_eq!(doc.toc_target(&storage, i), Some(Locator::new(i, 0)));
        }

        // Opened again, the cache gives back the same blocks; converted
        // afresh on another storage, too.
        let again = open(&storage);
        prop_assert_eq!(&blocks(&again, &storage), &first);
        let fresh = MemoryStorage::new().with("book.epub", epub);
        prop_assert_eq!(&blocks(&open(&fresh), &fresh), &first);
    }

    #[test]
    fn every_position_in_a_block_finds_that_block(
        chapters in chapters(),
        picks in prop::collection::vec(any::<prop::sample::Index>(), 1..20),
    ) {
        let bodies: Vec<String> = chapters.iter().map(|c| body(c)).collect();
        let storage = MemoryStorage::new().with("book.epub", book_with(&bodies, "", &[], false));
        let doc = open(&storage);
        let all = blocks(&doc, &storage);
        for pick in picks {
            let (section, block) = pick.get(&all);
            let span = block.end - block.start;
            for offset in [block.start, block.start + span / 2, block.end - 1] {
                let found = doc.block_at(&storage, Locator::new(*section, offset)).unwrap().unwrap();
                prop_assert_eq!(&found, block);
            }
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(
        data in prop::collection::vec(any::<u8>(), 0..2048),
        zip_start in any::<bool>(),
    ) {
        let mut data = data;
        if zip_start {
            data.splice(0..0, *b"PK\x03\x04");
        }
        exercise(data);
    }

    #[test]
    fn damaged_generated_books_never_panic(
        chapters in prop::collection::vec(prop::collection::vec(para(), 1..4), 1..3),
        stored in any::<bool>(),
        damage in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>(), 0u8..4), 1..6),
    ) {
        let bodies: Vec<String> = chapters.iter().map(|c| body(c)).collect();
        let mut epub = book_with(&bodies, ".x { text-align: right }", &[], stored);
        for (at, byte, how) in damage {
            let at = at.index(epub.len());
            match how {
                0 => epub[at] = byte,
                1 => epub[at] ^= 1 << (byte % 8),
                2 => epub.truncate(at.max(1)),
                _ => {
                    epub.insert(at, byte);
                }
            }
        }
        exercise(epub);
    }
}

/// Generated books: the same words wherever the book is read from.
#[test]
fn generated_words_survive_markup_characters() {
    // A fixed case of the property above, for a quick look when it fails.
    let paras = vec![Para {
        element: 3,
        words: vec![
            ("a&b".into(), 0),
            ("<c>".into(), 1),
            ("\"q\"".into(), 5),
            ("中€".into(), 4),
        ],
    }];
    let storage =
        MemoryStorage::new().with("book.epub", book_with(&[body(&paras)], "", &[], false));
    let doc = open(&storage);
    assert_eq!(words(&doc, &storage), ["a&b", "<c>", "\"q\"", "中€"]);
}
