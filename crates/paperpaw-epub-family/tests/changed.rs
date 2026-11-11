//! A book replaced by another of its name, even of the same size, is read
//! anew: nothing converted or decoded from the one before stays.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_epub_family::{EpubDocument, Fb2Document, HtmlDocument, MarkdownDocument};
use paperpaw_sdk::document::{BlockKind, Document, Locator};
use paperpaw_sdk::{MemoryStorage, OpenOptions, Storage};
use paperpaw_zip::testing::ZipBuilder;

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;

const OPF: &str = "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" \
    version=\"3.0\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
    <dc:title>Changed</dc:title></metadata><manifest>\
    <item id=\"c1\" href=\"ch1.xhtml\" media-type=\"application/xhtml+xml\"/>\
    </manifest><spine><itemref idref=\"c1\"/></spine></package>";

/// An EPUB whose chapter says `word` before a picture; stored, so books
/// of words of one length are of one size.
fn epub(word: &str) -> Vec<u8> {
    let chapter = format!("<html><body><p>{word}</p><img src=\"dot.png\" alt=\"\"/></body></html>");
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .stored("META-INF/container.xml", CONTAINER)
        .stored("OEBPS/content.opf", OPF)
        .stored("OEBPS/ch1.xhtml", chapter)
        .stored(
            "OEBPS/dot.png",
            &include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png")[..],
        )
        .build()
}

/// The texts of a document's blocks.
fn texts(doc: &dyn Document, storage: &dyn Storage) -> Vec<String> {
    let mut texts = Vec::new();
    let mut at = Locator::START;
    while let Some(block) = doc.block_at(storage, at).unwrap() {
        at = Locator::new(0, block.end);
        texts.push(block.text);
    }
    texts
}

#[test]
fn a_replaced_epub_is_converted_and_its_pictures_decoded_anew() {
    let (alpha, bravo) = (epub("Alpha"), epub("Bravo"));
    assert_eq!(alpha.len(), bravo.len());
    let storage = MemoryStorage::new().with("b/book.epub", alpha);
    let doc = EpubDocument::open(&storage, "b/book.epub").unwrap();
    assert_eq!(texts(&doc, &storage)[0], "Alpha");
    let picture = doc
        .block_at(&storage, Locator::new(0, 1))
        .unwrap()
        .and_then(|b| doc.block_at(&storage, Locator::new(0, b.end)).unwrap())
        .filter(|b| b.kind == BlockKind::Image)
        .and_then(|b| b.image)
        .unwrap();
    let decoded = doc.prepare_image(&storage, &picture).unwrap();
    assert!(storage.size(&decoded.path).is_ok());

    storage.write("b/book.epub", &bravo).unwrap();
    let doc = EpubDocument::open(&storage, "b/book.epub").unwrap();
    assert!(
        storage.size(&decoded.path).is_err(),
        "the old picture stays"
    );
    assert_eq!(texts(&doc, &storage)[0], "Bravo");
}

/// A picture decoded anew for a zoom: a file of its own beside the one
/// for the page, never larger than the picture, made once; gone with the
/// book's other files when the book is replaced. A picture the book does
/// not note is none.
#[test]
fn a_picture_is_decoded_larger_beside_the_one_for_its_page() {
    let storage = MemoryStorage::new().with("b/book.epub", epub("Alpha"));
    let doc = EpubDocument::open(&storage, "b/book.epub").unwrap();
    let picture = doc
        .block_at(&storage, Locator::new(0, 1))
        .unwrap()
        .and_then(|b| doc.block_at(&storage, Locator::new(0, b.end)).unwrap())
        .filter(|b| b.kind == BlockKind::Image)
        .and_then(|b| b.image)
        .unwrap();
    let page = doc
        .prepare_image(&storage, &picture)
        .unwrap_or(picture.clone());
    let larger = doc.prepare_image_larger(&storage, &picture, 1200).unwrap();
    assert_ne!(larger.path, page.path);
    assert!(larger.path.contains("-1200."), "{}", larger.path);
    assert!(storage.size(&larger.path).is_ok());
    // The picture is small: no larger than it is.
    assert!(larger.width <= 1200 && larger.height <= 1200);
    assert_eq!((larger.width, larger.height), (page.width, page.height));
    // Asked again: the same file.
    assert_eq!(
        doc.prepare_image_larger(&storage, &picture, 1200),
        Some(larger.clone())
    );
    // Larger than pictures are decoded at most: capped.
    let most = doc
        .prepare_image_larger(&storage, &picture, 99_999)
        .unwrap();
    assert!(most.path.contains("-1600."), "{}", most.path);
    // Not a picture of the book.
    let stranger = paperpaw_sdk::document::ImageRef {
        path: String::from("b/elsewhere/none.gray"),
        width: 1,
        height: 1,
    };
    assert_eq!(doc.prepare_image_larger(&storage, &stranger, 1200), None);
    // Replaced: gone with the rest.
    storage.write("b/book.epub", &epub("Bravo")).unwrap();
    let _ = EpubDocument::open(&storage, "b/book.epub").unwrap();
    assert!(storage.size(&larger.path).is_err());
}

#[test]
fn chapters_converted_with_stylesheets_one_has_no_more_go() {
    let storage = MemoryStorage::new().with("b/book.epub", epub("Alpha"));
    for css in ["p { text-align: left }", "p { text-align: right }"] {
        let options = OpenOptions::default().css(String::from(css));
        let doc = EpubDocument::open_options(&storage, "b/book.epub", &options).unwrap();
        doc.block_at(&storage, Locator::START).unwrap();
    }
    let mut folders = 0;
    let cache =
        paperpaw_sdk::library::kept_path(".paperpaw/plugins/format-epub", "", "b/book.epub");
    storage
        .list(&cache, &mut |entry| {
            folders += usize::from(entry.is_dir && entry.name.starts_with("css-"));
        })
        .unwrap();
    assert_eq!(folders, 1);
}

/// The text of a document's last block.
fn last(doc: &dyn Document, storage: &dyn Storage) -> String {
    texts(doc, storage).pop().unwrap_or_default()
}

/// A text long enough that its first 4 KB say nothing of its end, which
/// says `word`.
fn long(before: &str, word: &str, after: &str) -> Vec<u8> {
    let filler = "Words that stay the same in both. ".repeat(200);
    format!("{before}{filler}{word}{after}").into_bytes()
}

#[test]
fn a_replaced_fictionbook_is_read_anew() {
    let book = |word| {
        long(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook><body><section><p>",
            word,
            "</p></section></body></FictionBook>",
        )
    };
    let storage = MemoryStorage::new().with("b/book.fb2", book("Alpha"));
    let doc = Fb2Document::open(&storage, "b/book.fb2", true).unwrap();
    assert!(last(&doc, &storage).ends_with("Alpha"));
    storage.write("b/book.fb2", &book("Bravo")).unwrap();
    let doc = Fb2Document::open(&storage, "b/book.fb2", true).unwrap();
    assert!(last(&doc, &storage).ends_with("Bravo"));
}

#[test]
fn a_replaced_web_page_is_read_anew() {
    let page = |word| long("<html><body><p>", word, "</p></body></html>");
    let storage = MemoryStorage::new().with("b/page.html", page("Alpha"));
    let doc = HtmlDocument::open(&storage, "b/page.html", true).unwrap();
    assert!(last(&doc, &storage).ends_with("Alpha"));
    storage.write("b/page.html", &page("Bravo")).unwrap();
    let doc = HtmlDocument::open(&storage, "b/page.html", true).unwrap();
    assert!(last(&doc, &storage).ends_with("Bravo"));
}

#[test]
fn a_replaced_markdown_file_is_read_anew() {
    let notes = |word| long("# Notes\n\n", word, "\n");
    let storage = MemoryStorage::new().with("b/notes.md", notes("Alpha"));
    let doc = MarkdownDocument::open(&storage, "b/notes.md", true).unwrap();
    assert!(last(&doc, &storage).ends_with("Alpha"));
    storage.write("b/notes.md", &notes("Bravo")).unwrap();
    let doc = MarkdownDocument::open(&storage, "b/notes.md", true).unwrap();
    assert!(last(&doc, &storage).ends_with("Bravo"));
}
