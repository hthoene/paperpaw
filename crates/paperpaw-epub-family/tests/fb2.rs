//! Hostile FictionBooks: broken base64 pictures, sections nested very
//! deep, notes and links leading nowhere, encodings as the XML prolog
//! declares them (or misdeclares them), files cut short, zipped books
//! without a book inside.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Recording, blocks, text, words};
use paperpaw_plugin_format_epub::Fb2Document;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{BlockKind, Document, DocumentError, Locator};
use paperpaw_zip::testing::ZipBuilder;

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A FictionBook with `description` and `body`, and `binaries` after.
fn fb2(description: &str, body: &str, binaries: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FictionBook \
         xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" \
         xmlns:l=\"http://www.w3.org/1999/xlink\"><description><title-info>{description}\
         </title-info></description><body>{body}</body>{binaries}</FictionBook>"
    )
}

fn open(data: impl Into<Vec<u8>>) -> (MemoryStorage, Fb2Document) {
    let storage = MemoryStorage::new().with("b/book.fb2", data);
    let doc = Fb2Document::open(&storage, "b/book.fb2", true).unwrap();
    (storage, doc)
}

#[test]
fn broken_binaries_give_no_picture_and_no_panic() {
    let good = base64(PNG);
    let binaries = format!(
        "<binary id=\"good\" content-type=\"image/png\">{good}</binary>\
         <binary id=\"garbage\" content-type=\"image/png\">!!!! not base64 at all ****</binary>\
         <binary id=\"cut\" content-type=\"image/png\">{}</binary>\
         <binary id=\"empty\" content-type=\"image/png\"></binary>\
         <binary id=\"padding\" content-type=\"image/png\">====</binary>\
         <binary content-type=\"image/png\">{good}</binary>",
        &good[..good.len() / 3]
    );
    let body = "<section><p>Pictures</p>\
        <image l:href=\"#good\"/><image l:href=\"#garbage\"/><image l:href=\"#cut\"/>\
        <image l:href=\"#empty\"/><image l:href=\"#padding\"/><image l:href=\"#missing\"/>\
        <image l:href=\"\"/><image/><p>After</p></section>";
    let (storage, doc) = open(fb2(
        "<book-title>Pics</book-title><coverpage><image l:href=\"#garbage\"/></coverpage>",
        body,
        &binaries,
    ));
    let read = blocks(&doc, &storage);
    assert_eq!(read.first().unwrap().1.text, "Pictures");
    assert_eq!(read.last().unwrap().1.text, "After");
    let pictures: Vec<_> = read
        .iter()
        .filter(|(_, b)| b.kind == BlockKind::Image)
        .collect();
    // Only the good one is a picture, decoded from the first binary of
    // its id.
    assert_eq!(pictures.len(), 1, "{read:?}");
    let image = pictures[0].1.image.as_ref().unwrap();
    assert!(image.width > 0 && image.height > 0);
    assert_eq!(doc.cover(&storage).unwrap(), None);
}

#[test]
fn a_second_binary_of_the_same_id_does_not_undo_the_first() {
    let (storage, doc) = open(fb2(
        "",
        "<section><image l:href=\"#pic\"/></section>",
        &format!(
            "<binary id=\"pic\">{}</binary><binary id=\"pic\">AAAA</binary>",
            base64(PNG)
        ),
    ));
    let read = blocks(&doc, &storage);
    assert_eq!(read[0].1.kind, BlockKind::Image);
}

#[test]
fn a_cover_that_decodes_is_found() {
    let (storage, doc) = open(fb2(
        "<coverpage><image l:href=\"#c\"/></coverpage>",
        "<section><p>x</p></section>",
        &format!("<binary id=\"c\">{}</binary>", base64(PNG)),
    ));
    let cover = doc.cover(&storage).unwrap().unwrap();
    assert!(cover.width > 0 && cover.height > 0);
}

#[test]
fn sections_nested_very_deep_keep_their_text_and_contents() {
    let depth = 1500;
    let mut body = String::new();
    for i in 0..depth {
        body.push_str(&format!("<section><title><p>T{i}</p></title><p>P{i}</p>"));
    }
    body.push_str(&"</section>".repeat(depth));
    let (storage, doc) = open(fb2("", &body, ""));
    let words = words(&doc, &storage);
    assert_eq!(words.len(), 2 * depth);
    assert_eq!(words[2 * depth - 1], format!("P{}", depth - 1));
    // Headings go no deeper than six, contents entries no deeper than
    // five.
    let read = blocks(&doc, &storage);
    assert!(read.iter().all(|(_, b)| match b.kind {
        BlockKind::Heading(level) => (1..=6).contains(&level),
        _ => true,
    }));
    assert_eq!(doc.toc_len(&storage) as usize, depth);
    for i in [0, 1, 5, 6, 700, depth as u32 - 1] {
        let entry = doc.toc_entry(&storage, i).unwrap();
        assert_eq!(entry.title, format!("T{i}"));
        assert!(entry.level <= 5);
        let block = doc.block_at(&storage, entry.at).unwrap().unwrap();
        assert_eq!(block.text, format!("T{i}"));
    }
}

#[test]
fn empty_section_titles_do_not_shift_the_contents() {
    // `<title/>` gets an anchor while converting but no contents entry
    // while scanning, so entry n leads to anchor n of another section.
    let body = "<section><title/><p>First text</p></section>\
                <section><title><p>Second</p></title><p>Second text</p></section>";
    let (storage, doc) = open(fb2("", body, ""));
    let titles: Vec<String> = (0..doc.toc_len(&storage))
        .map(|i| doc.toc_entry(&storage, i).unwrap().title)
        .collect();
    let second = titles.iter().position(|t| t == "Second").unwrap() as u32;
    let at = doc.toc_target(&storage, second).unwrap();
    assert_eq!(doc.block_at(&storage, at).unwrap().unwrap().text, "Second");
}

#[test]
fn links_and_notes_leading_nowhere_find_nothing() {
    let body = "<section><p>See <a l:href=\"#n1\" type=\"note\">1</a>, \
        <a l:href=\"#nowhere\">2</a>, <a l:href=\"#\">3</a>, \
        <a l:href=\"https://example.com/\">web</a>, <a>bare</a>.</p></section>";
    let notes = "<body name=\"notes\"><section id=\"n1\"><p>The note.</p></section></body>";
    let book = fb2("", body, notes);
    let (storage, doc) = open(book);
    let links = doc.links(&storage, 0, 0, u32::MAX);
    let hrefs: Vec<&str> = links.iter().map(|l| l.href.as_str()).collect();
    assert_eq!(
        hrefs,
        [
            "b/book.fb2#n1",
            "b/book.fb2#nowhere",
            "b/book.fb2#",
            "https://example.com/"
        ]
    );
    let note = doc.link_target(&storage, hrefs[0]).unwrap();
    assert_eq!(
        doc.block_at(&storage, note).unwrap().unwrap().text,
        "The note."
    );
    assert_eq!(doc.link_target(&storage, hrefs[1]), None);
    assert_eq!(doc.link_target(&storage, hrefs[2]), None);
    assert_eq!(doc.link_target(&storage, hrefs[3]), None);
    assert_eq!(doc.links(&storage, 1, 0, u32::MAX), Vec::new());
    assert_eq!(doc.block_at(&storage, Locator::new(1, 0)).unwrap(), None);
}

/// A book in a single-byte encoding: its prolog says `declared`, its
/// title and text are `bytes`.
fn encoded(declared: &str, bytes: &[u8]) -> Vec<u8> {
    let mut book = format!("<?xml version=\"1.0\" encoding={declared}?><FictionBook><description><title-info><book-title>")
        .into_bytes();
    book.extend_from_slice(bytes);
    book.extend_from_slice(b"</book-title></title-info></description><body><section><p>");
    book.extend_from_slice(bytes);
    book.extend_from_slice(b"</p></section></body></FictionBook>");
    book
}

#[test]
fn encodings_are_read_as_the_prolog_declares() {
    // "Größe €" in Windows-1252, "Мир" in Windows-1251.
    let western: &[u8] = &[b'G', b'r', 0xf6, 0xdf, b'e', b' ', 0x80];
    let cyrillic: &[u8] = &[0xcc, 0xe8, 0xf0];
    for (declared, bytes, expected) in [
        ("\"windows-1252\"", western, "Größe €"),
        ("'WINDOWS-1252'", western, "Größe €"),
        ("\"cp1252\"", western, "Größe €"),
        ("\"ISO-8859-1\"", western, "Größe €"),
        ("\"latin1\"", western, "Größe €"),
        ("\"windows-1251\"", cyrillic, "Мир"),
        ("'cp1251'", cyrillic, "Мир"),
        ("\"utf-8\"", "Größe".as_bytes(), "Größe"),
    ] {
        let (storage, doc) = open(encoded(declared, bytes));
        assert_eq!(
            doc.metadata().title.as_deref(),
            Some(expected),
            "{declared}"
        );
        assert_eq!(text(&doc, &storage), expected, "{declared}");
    }
}

#[test]
fn unknown_or_wrong_encodings_read_as_utf8_without_panic() {
    // KOI8-R is not read: as UTF-8, its bytes are not text.
    for (declared, bytes) in [
        ("\"koi8-r\"", &[0xed, 0xc9, 0xd2][..]),
        ("\"utf-8\"", &[0xff, 0xfe, 0xc3][..]),
        ("\"\"", &b"ok"[..]),
        ("\"windows-1251", &[0xcc][..]),
    ] {
        let (storage, doc) = open(encoded(declared, bytes));
        let read = text(&doc, &storage);
        assert!(!read.is_empty(), "{declared}");
    }
    // UTF-16 is not read either, but opens.
    let utf16: Vec<u8> = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-16\"?><FictionBook/>"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let storage = MemoryStorage::new().with("u.fb2", utf16);
    if let Ok(doc) = Fb2Document::open(&storage, "u.fb2", true) {
        blocks(&doc, &storage);
    }
}

#[test]
fn a_book_cut_short_keeps_what_it_has() {
    let book = fb2(
        "<book-title>Cut</book-title>",
        "<section><title><p>One</p></title><p>First paragraph.</p><p>Second paragraph.</p></section>",
        "",
    );
    for cut in [
        book.find("First").unwrap() + 12,
        book.find("Second").unwrap() + 3,
        book.len() - 20,
    ] {
        let (storage, doc) = open(&book.as_bytes()[..cut]);
        let words = words(&doc, &storage);
        assert_eq!(
            words.first().map(String::as_str),
            Some("One"),
            "cut at {cut}"
        );
        assert!(words.len() >= 2);
    }
}

#[test]
fn zipped_books_without_a_book_or_with_odd_names() {
    let storage = Recording::new(
        MemoryStorage::new()
            .with(
                "none.fb2.zip",
                ZipBuilder::new()
                    .deflated("readme.txt", "no book here")
                    .build(),
            )
            .with("broken.fb2.zip", b"PK\x03\x04 not a zip".to_vec())
            .with(
                "odd.fb2.zip",
                ZipBuilder::new()
                    .deflated(
                        "../../../evil.FB2",
                        fb2("<book-title>Odd</book-title>", "<p>x</p>", ""),
                    )
                    .build(),
            ),
    );
    assert!(matches!(
        Fb2Document::open(&storage, "none.fb2.zip", true),
        Err(DocumentError::Malformed(_))
    ));
    assert!(Fb2Document::open(&storage, "broken.fb2.zip", true).is_err());
    let doc = Fb2Document::open(&storage, "odd.fb2.zip", true).unwrap();
    assert_eq!(doc.metadata().title.as_deref(), Some("Odd"));
    assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
}

#[test]
fn metadata_of_any_size_is_bounded() {
    let long = "word ".repeat(100_000);
    let (_, doc) = open(fb2(
        &format!(
            "<book-title>{long}</book-title><annotation><p>{long}</p></annotation>\
             <author><nickname>Nick</nickname></author><author></author>\
             <sequence name=\"\" number=\"0\"/>"
        ),
        "<section><p>x</p></section>",
        "",
    ));
    let m = doc.metadata();
    // About 4 KB each, give or take a piece of text.
    assert!(m.title.as_ref().unwrap().len() <= 8192);
    assert!(m.description.as_ref().unwrap().len() <= 8192);
    assert_eq!(m.authors, ["Nick"]);
    assert_eq!(m.series, None);
}
