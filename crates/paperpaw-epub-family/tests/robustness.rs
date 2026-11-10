//! Damaged EPUBs must fail cleanly: no panic, no endless loop, bounded
//! memory. Real books are mutated at random (deterministically, so a
//! failure can be reproduced from its seed):
//!
//! - bytes of the file, with a bias towards the ZIP structures at both
//!   ends;
//! - the text of the package, navigation and chapter documents, which are
//!   then packed into a valid archive again, so the damage reaches the XML
//!   tokenizer, the package reader and the converter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::zip::ZipArchive;
use paperpaw_zip::testing::ZipBuilder;

const BOOKS: [&[u8]; 2] = [
    include_bytes!("../../../samples/library/books/alice-in-wonderland.epub"),
    include_bytes!("../../../samples/library/books/die-verwandlung.epub"),
];

/// xorshift64*: small, deterministic, good enough for mutations.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Changes a few bytes of the file; rarely its length.
fn mutate(book: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut data = book.to_vec();
    for _ in 0..1 + rng.below(3) {
        let len = data.len();
        // Where: the central directory, the first local headers, anywhere.
        let at = match rng.below(3) {
            0 => len - 1 - rng.below(4096.min(len)),
            1 => rng.below(2048.min(len)),
            _ => rng.below(len),
        };
        match rng.below(16) {
            0..=4 => data[at] ^= 1 << rng.below(8),
            5..=8 => data[at] = rng.next() as u8,
            9..=12 => data[at] = [0x00, 0xff, 0x7f, 0x80][rng.below(4)],
            13 => data.truncate(at.max(1)),
            14 => {
                let end = (at + 1 + rng.below(64)).min(data.len());
                data.drain(at..end);
            }
            _ => {
                let bytes: Vec<u8> = (0..1 + rng.below(16)).map(|_| rng.next() as u8).collect();
                data.splice(at..at, bytes);
            }
        }
    }
    data
}

/// Damages markup: drops or doubles characters around tags and quotes,
/// inserts stray tags, entities and long runs.
fn mutate_text(text: &str, rng: &mut Rng) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    const PIECES: [&str; 12] = [
        "<",
        ">",
        "\"",
        "</p>",
        "<b><i>",
        "</i></b></b>",
        "&amp",
        "&#99999999;",
        "<div style=\"text-align:center;font-weight:900\" class=\".a  b\">",
        "<br",
        "]]>",
        "<![CDATA[",
    ];
    for _ in 0..1 + rng.below(12) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(5) {
            0 => {
                chars.remove(at);
            }
            1 => {
                let end = (at + 1 + rng.below(200)).min(chars.len());
                chars.drain(at..end);
            }
            2 => {
                let c = chars[at];
                chars.insert(at, c);
            }
            3 => {
                let piece = PIECES[rng.below(PIECES.len())];
                for (k, c) in piece.chars().enumerate() {
                    chars.insert(at + k, c);
                }
            }
            _ => {
                let c = ['x', ' ', '\u{ad}', '\u{2028}', 'ß', '\n'][rng.below(6)];
                for _ in 0..rng.below(3000) {
                    chars.insert(at, c);
                }
            }
        }
    }
    chars.into_iter().collect()
}

/// Repacks the book with some of its XML documents damaged.
fn mutate_content(book: &[u8], rng: &mut Rng) -> Vec<u8> {
    let storage = MemoryStorage::new().with("book.epub", book.to_vec());
    let archive = ZipArchive::open(&storage, "book.epub").expect("sample books are valid");
    let mut builder = ZipBuilder::new();
    let mut entries: Vec<_> = archive.entries(&storage).map(|e| e.unwrap()).collect();
    // The mimetype stays first, as EPUB requires.
    entries.sort_by_key(|e| (e.name != "mimetype", e.name.clone()));
    for entry in entries {
        let data = archive.read(&storage, &entry, 4 << 20).unwrap();
        let xml = [".xhtml", ".html", ".opf", ".ncx", ".xml"]
            .iter()
            .any(|ext| entry.name.ends_with(ext));
        builder = if xml && rng.below(3) == 0 {
            let text = String::from_utf8_lossy(&data);
            builder.deflated(&entry.name, mutate_text(&text, rng))
        } else if entry.name == "mimetype" {
            builder.stored(&entry.name, data)
        } else {
            builder.deflated(&entry.name, data)
        };
    }
    builder.build()
}

/// Opens and reads a book as a reader would, within bounds; returns the
/// number of blocks read.
fn exercise(data: Vec<u8>) -> usize {
    let storage = MemoryStorage::new().with("book.epub", data);
    let Ok(doc) = EpubDocument::open(&storage, "book.epub") else {
        return 0;
    };
    let mut blocks = 0;
    let _ = doc.metadata();
    let sections = doc.sections().min(4);
    for section in 0..sections {
        let mut at = Locator::new(section, 0);
        for _ in 0..5000 {
            match doc.block_at(&storage, at) {
                Ok(Some(block)) => {
                    assert!(block.end > at.offset, "no progress at {at:?}");
                    assert!(block.text.len() <= 8192);
                    let _ = (block.style_at(0), doc.progress(at));
                    at = Locator::new(section, block.end);
                    blocks += 1;
                }
                _ => break,
            }
        }
    }
    let toc = doc.toc_len(&storage).min(50);
    for i in 0..toc {
        let _ = doc.toc_entry(&storage, i);
        let _ = doc.toc_target(&storage, i);
    }
    blocks
}

#[test]
fn damaged_books_fail_cleanly() {
    // PAPERPAW_FUZZ_ITERATIONS raises the count for longer local runs.
    let iterations: u64 = std::env::var("PAPERPAW_FUZZ_ITERATIONS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(150);
    let mut readable = 0;
    for seed in 1..=iterations {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let book = BOOKS[(seed % 2) as usize];
        let data = mutate(book, &mut rng);
        let result = std::panic::catch_unwind(|| exercise(data));
        match result {
            Ok(blocks) => readable += usize::from(blocks > 0),
            Err(_) => panic!("seed {seed} panicked"),
        }
    }
    // Most damage hits compressed data of single chapters; many books must
    // still open, or the mutations only test the first check.
    assert!(
        readable as u64 * 4 >= iterations,
        "only {readable} readable"
    );
}

#[test]
fn damaged_documents_fail_cleanly() {
    let iterations: u64 = std::env::var("PAPERPAW_FUZZ_ITERATIONS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(40);
    let mut readable = 0;
    for seed in 1..=iterations {
        let mut rng = Rng(seed.wrapping_mul(0xd1b5_4a32_d192_ed03));
        let book = BOOKS[(seed % 2) as usize];
        let data = mutate_content(book, &mut rng);
        let result = std::panic::catch_unwind(|| exercise(data));
        match result {
            Ok(blocks) => readable += usize::from(blocks > 0),
            Err(_) => panic!("content seed {seed} panicked"),
        }
    }
    assert!(
        readable as u64 * 2 >= iterations,
        "only {readable} readable"
    );
}

#[test]
fn unmodified_books_still_read() {
    for book in BOOKS {
        assert!(exercise(book.to_vec()) > 20);
    }
}

/// Reads any document as a reader would, within bounds.
fn read_document(doc: &dyn Document, storage: &MemoryStorage) {
    for section in 0..doc.sections().min(4) {
        let mut at = Locator::new(section, 0);
        for _ in 0..5000 {
            match doc.block_at(storage, at) {
                Ok(Some(block)) => {
                    assert!(block.end > at.offset, "no progress at {at:?}");
                    at = Locator::new(section, block.end);
                }
                _ => break,
            }
        }
    }
    for i in 0..doc.toc_len(storage).min(50) {
        let _ = doc.toc_entry(storage, i);
        let _ = doc.toc_target(storage, i);
    }
    let _ = doc.cover(storage);
}

const FB2: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns:l="http://www.w3.org/1999/xlink"><description><title-info>
<author><first-name>Ann</first-name><last-name>Example</last-name></author>
<book-title>Damaged</book-title><coverpage><image l:href="#c"/></coverpage>
</title-info></description><body><title><p>Damaged</p></title>
<section><title><p>One</p></title><p>Some <emphasis>text</emphasis><a l:href="#n1">1</a>.</p>
<section><title><p>Inner</p></title><p>More <style name="x">styled</style> text.</p></section>
</section><section><title><p>Two</p></title><poem><stanza><v>A line</v></stanza></poem></section>
</body><body name="notes"><section id="n1"><p>A note.</p></section></body>
<binary id="c" content-type="image/png">iVBORw0KGgo=</binary></FictionBook>"##;

const HTML: &str = "<!DOCTYPE html><html lang=\"en\"><head><title>Page</title>\
    <style>.x { text-align: center }</style></head><body><nav>Menu</nav><main>\
    <h1>Title</h1><p class=\"x\">A <a href=\"#n\">link</a><p>Open<ul><li>One<li>Two</ul>\
    <table><tr><td>A<td>B</table><p id=\"n\">Note</main></body></html>";

const MARKDOWN: &str = "# Title\n\nSome *text* and **more** with `code`.\n\n\
    - one\n- [a link](https://example.org)\n\n> quoted\n\n```\ncode\n```\n\n\
    ![picture](p.png)\n\n1. first\n2. second\n";

#[test]
fn damaged_fictionbooks_web_pages_and_markdown_fail_cleanly() {
    use paperpaw_plugin_format_epub::{Fb2Document, HtmlDocument, MarkdownDocument};
    let iterations: u64 = std::env::var("PAPERPAW_FUZZ_ITERATIONS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(60);
    for seed in 1..=iterations {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let (fb2, html, markdown) = (
            mutate_text(FB2, &mut rng),
            mutate_text(HTML, &mut rng),
            mutate_text(MARKDOWN, &mut rng),
        );
        let result = std::panic::catch_unwind(|| {
            let storage = MemoryStorage::new()
                .with("b/book.fb2", fb2)
                .with("b/page.html", html)
                .with("b/notes.md", markdown);
            if let Ok(doc) = Fb2Document::open(&storage, "b/book.fb2", true) {
                read_document(&doc, &storage);
            }
            if let Ok(doc) = HtmlDocument::open(&storage, "b/page.html", true) {
                read_document(&doc, &storage);
            }
            if let Ok(doc) = MarkdownDocument::open(&storage, "b/notes.md", true) {
                read_document(&doc, &storage);
            }
        });
        assert!(result.is_ok(), "seed {seed} panicked");
    }
}
