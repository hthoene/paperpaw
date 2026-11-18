//! Mobipocket books through the document's own reading of a converted cache: metadata, blocks,
//! styles, links, contents, pictures, and what is refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Builder, png};
use paperpaw_plugin_format_mobi::MobiDocument;
use paperpaw_sdk::document::{BlockKind, Document, DocumentError, Locator};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::{MemoryStorage, OpenOptions};

const PATH: &str = "books/test.mobi";
const CACHE: &str = ".paperpaw/plugins/format-mobi/book";

fn options() -> OpenOptions {
    OpenOptions::default().image_fit(Fit::screen(480, 800))
}

fn open(builder: &Builder) -> (MemoryStorage, MobiDocument) {
    let storage = MemoryStorage::new().with(PATH, builder.build());
    let document = MobiDocument::open_converted(&storage, PATH, CACHE, &options()).unwrap();
    (storage, document)
}

/// Every block in order: (kind, text).
fn blocks(storage: &MemoryStorage, document: &MobiDocument) -> Vec<(BlockKind, String)> {
    let mut at = Locator::new(0, 0);
    let mut all = Vec::new();
    while let Some(block) = document.block_at(storage, at).unwrap() {
        at = Locator::new(0, block.end);
        all.push((block.kind, block.text));
        assert!(all.len() < 10_000);
    }
    all
}

const BOOK: &str = "<html><head><guide><reference type=\"toc\" filepos=0000000000 /></guide>\
    <title>Ignored</title></head><body>\
    <h1>The Title</h1>\
    <p>Hello   <b>bold</b> and <i>it</i>.\n  Next &amp; last &#233;&eacute;&unknown; &lt;ok&gt;</p>\
    <p align=\"center\">Middle</p>\
    <p>One<br/>Two<BR>Three</p>\
    <h2>Second</h2>\
    <ul><li>first</li><li>second</li></ul>\
    <blockquote><p>   </p></blockquote><hr/>\
    </body></html>";

#[test]
fn metadata_comes_from_the_headers() {
    let mut builder = Builder::new(BOOK);
    builder.authors = vec!["Ann Author".into(), "Bo Writer".into()];
    builder.subjects = vec!["Fiction".into()];
    builder.language = Some("de".into());
    let (_, document) = open(&builder);
    let m = document.metadata();
    assert_eq!(m.title.as_deref(), Some("A Test Book"));
    assert_eq!(m.authors, ["Ann Author", "Bo Writer"]);
    assert_eq!(m.language.as_deref(), Some("de"));
    assert_eq!(m.subjects, ["Fiction"]);
    assert_eq!(document.sections(), 1);
    assert_eq!(document.section_len(0), BOOK.len() as u32);
}

#[test]
fn text_headings_and_spacing() {
    let (storage, document) = open(&Builder::new(BOOK));
    let all = blocks(&storage, &document);
    let texts: Vec<&str> = all.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        texts,
        [
            "The Title",
            "Hello bold and it. Next & last éé&unknown; <ok>",
            "Middle",
            "One\nTwo\nThree",
            "Second",
            "• first",
            "• second",
            "* * *",
        ]
    );
    assert_eq!(all[0].0, BlockKind::Heading(1));
    assert_eq!(all[4].0, BlockKind::Heading(2));
    assert_eq!(all[1].0, BlockKind::Paragraph);
}

#[test]
fn styles_and_alignment() {
    let (storage, document) = open(&Builder::new(BOOK));
    let block = document
        .block_at(
            &storage,
            Locator::new(0, BOOK.find("Hello").unwrap() as u32),
        )
        .unwrap()
        .unwrap();
    let styled: Vec<(&str, u32)> = block
        .runs
        .iter()
        .map(|r| {
            (
                &block.text[r.start as usize..r.end as usize],
                r.style.bits(),
            )
        })
        .collect();
    assert_eq!(styled, [("bold", 1), ("it", 2)]);
    let middle = document
        .block_at(
            &storage,
            Locator::new(0, BOOK.find("Middle").unwrap() as u32),
        )
        .unwrap()
        .unwrap();
    assert_eq!(middle.align, paperpaw_sdk::document::Align::Center);
    // `<br>` and a blank paragraph change no alignment of the next.
    assert_eq!(block.align, paperpaw_sdk::document::Align::Auto);
}

#[test]
fn every_offset_of_a_block_gives_the_block() {
    let (storage, document) = open(&Builder::new(BOOK));
    let mut at = 0u32;
    let mut last_end = 0u32;
    while let Some(block) = document.block_at(&storage, Locator::new(0, at)).unwrap() {
        assert!(block.start >= last_end, "blocks keep to their places");
        assert!(block.end > block.start);
        for inside in [block.start, (block.start + block.end) / 2, block.end - 1] {
            let again = document
                .block_at(&storage, Locator::new(0, inside))
                .unwrap()
                .unwrap();
            assert_eq!(
                (again.start, again.end, &again.text),
                (block.start, block.end, &block.text)
            );
        }
        last_end = block.end;
        at = block.end;
    }
    assert!(
        document
            .block_at(&storage, Locator::new(1, 0))
            .unwrap()
            .is_none()
    );
    assert!(
        document
            .block_at(&storage, Locator::new(0, u32::MAX))
            .unwrap()
            .is_none()
    );
}

#[test]
fn compressed_and_plain_books_read_the_same() {
    let long = format!(
        "<p>{}</p>",
        "The wind came over the hills and over the sea. ".repeat(400)
    );
    let html = format!("{BOOK}{long}{BOOK}");
    let mut plain = Builder::new(&html);
    plain.compress = false;
    let (s1, d1) = open(&plain);
    let (s2, d2) = open(&Builder::new(&html));
    let (a, b) = (blocks(&s1, &d1), blocks(&s2, &d2));
    assert!(a.len() > 20, "a long paragraph is cut in pieces");
    assert_eq!(a, b);
    // The pieces of a long paragraph say so.
    let piece = d1
        .block_at(&s1, Locator::new(0, (BOOK.len() + 100) as u32))
        .unwrap()
        .unwrap();
    assert!(piece.continues && !piece.continued);
}

#[test]
fn trailing_data_of_records_is_not_text() {
    let html = format!("<p>{}</p>", "Alpha beta gamma. ".repeat(600));
    let mut builder = Builder::new(&html);
    builder.trailing = true;
    let (storage, document) = open(&builder);
    let all = blocks(&storage, &document);
    let text: String = all
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(!text.contains('\u{7}'));
    assert!(text.starts_with("Alpha beta gamma. Alpha"));
    assert_eq!(text.matches("Alpha").count(), 600);
}

#[test]
fn windows_1252_books() {
    let mut builder = Builder::bytes(b"<p>caf\xe9 \x93quoted\x94 \x80 5</p>");
    builder.utf8 = false;
    builder.title = "Caf\u{e9}".into();
    let (storage, document) = open(&builder);
    assert_eq!(blocks(&storage, &document)[0].1, "café “quoted” € 5");
    // UTF-8 characters cut by the end of a record are put together.
    let html = format!("<p>{}</p>", "é".repeat(5000));
    let (storage, document) = open(&Builder::new(&html));
    let text: String = blocks(&storage, &document)
        .iter()
        .map(|(_, t)| t.clone())
        .collect();
    assert_eq!(text.chars().filter(|&c| c == 'é').count(), 5000);
    assert!(!text.contains('\u{fffd}'));
}

#[test]
fn links_lead_to_positions() {
    let target = "<p>Here it is.</p>";
    let head = "<body><p>See <a filepos=0000000000>the end</a> now.</p>";
    let html_without = format!("{head}{target}");
    let filepos = html_without.find(target).unwrap();
    let html = html_without.replace("0000000000", &format!("{filepos:010}"));
    let (storage, document) = open(&Builder::new(&html));
    let block = document
        .block_at(&storage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    assert_eq!(block.text, "See the end now.");
    let links = document.links(&storage, 0, block.start, block.end);
    assert_eq!(links.len(), 1);
    let at = document.link_target(&storage, &links[0].href).unwrap();
    assert_eq!(at, Locator::new(0, filepos as u32));
    let there = document.block_at(&storage, at).unwrap().unwrap();
    assert_eq!(there.text, "Here it is.");
    // The link covers its words and no others.
    let covered = &html[links[0].start as usize..links[0].end as usize];
    assert_eq!(covered, "the end");
    assert!(document.link_target(&storage, "#99999999").is_none());
    assert!(document.link_target(&storage, "http://x").is_none());
    assert!(
        document
            .links(&storage, 0, there.start, there.end)
            .is_empty()
    );
}

#[test]
fn the_contents_are_the_headings() {
    let (storage, document) = open(&Builder::new(BOOK));
    assert_eq!(document.toc_len(&storage), 2);
    let first = document.toc_entry(&storage, 0).unwrap();
    assert_eq!((first.title.as_str(), first.level), ("The Title", 0));
    let second = document.toc_entry(&storage, 1).unwrap();
    assert_eq!((second.title.as_str(), second.level), ("Second", 1));
    let block = document.block_at(&storage, second.at).unwrap().unwrap();
    assert_eq!(block.text, "Second");
    assert!(document.toc_entry(&storage, 2).is_none());
}

#[test]
fn pictures_are_decoded_when_their_page_shows() {
    let mut builder = Builder::new(
        "<p>Before <img recindex=\"00001\" alt=\"A  grey\n gradient\"/>after</p><p><img recindex=00002 /></p><img recindex=00009 />",
    );
    builder.pictures = vec![png(40, 30), b"not a picture".to_vec()];
    builder.cover = Some(0);
    let (storage, document) = open(&builder);
    let all = blocks(&storage, &document);
    let kinds: Vec<BlockKind> = all.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        [BlockKind::Paragraph, BlockKind::Image, BlockKind::Paragraph]
    );
    assert_eq!(all[0].1, "Before");
    assert_eq!(all[1].1, "A grey gradient");
    assert_eq!(all[2].1, "after");
    let first = document
        .block_at(&storage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    let block = document
        .block_at(&storage, Locator::new(0, first.end))
        .unwrap()
        .unwrap();
    let image = block.image.clone().expect("a picture");
    assert_eq!((image.width, image.height), (40, 30));
    // Decoded on its page, once.
    let ready = document.prepare_image(&storage, &image).expect("decoded");
    assert_eq!((ready.width, ready.height), (40, 30));
    assert!(document.prepare_image(&storage, &ready).is_none());
    // The cover is the first picture.
    let cover = document.cover(&storage).unwrap().expect("a cover");
    assert_eq!((cover.width, cover.height), (40, 30));
    assert!(block.continued || !block.continued);
}

#[test]
fn a_picture_that_cannot_be_decoded_is_tried_once() {
    let mut builder = Builder::new("<p><img recindex=1 /></p>");
    let mut broken = png(40, 30);
    broken.truncate(60);
    builder.pictures = vec![broken];
    let (storage, document) = open(&builder);
    // Its size is told by its header, so it is a block; decoding fails.
    if let Some(block) = document.block_at(&storage, Locator::new(0, 0)).unwrap()
        && let Some(image) = block.image
    {
        assert!(document.prepare_image(&storage, &image).is_none());
        assert!(document.prepare_image(&storage, &image).is_none());
    }
}

fn refused(builder: &Builder) -> String {
    let storage = MemoryStorage::new().with(PATH, builder.build());
    match MobiDocument::open_converted(&storage, PATH, CACHE, &options()) {
        Err(DocumentError::Malformed(why)) => why.into_owned(),
        Err(other) => format!("{other}"),
        Ok(_) => String::from("opened"),
    }
}

#[test]
fn books_that_cannot_be_read_say_why() {
    let mut drm = Builder::new(BOOK);
    drm.encryption = 2;
    assert!(refused(&drm).contains("DRM"), "{}", refused(&drm));
    let mut huff = Builder::new(BOOK);
    huff.compression = Some(17480);
    assert!(refused(&huff).contains("Huffman"), "{}", refused(&huff));
    let mut kf8 = Builder::new(BOOK);
    kf8.version = 8;
    assert!(refused(&kf8).contains("KF8"), "{}", refused(&kf8));
    let mut odd = Builder::new(BOOK);
    odd.compression = Some(7);
    assert!(refused(&odd).contains("compression"));
    let storage = MemoryStorage::new().with(PATH, b"just some text".to_vec());
    assert!(MobiDocument::open_converted(&storage, PATH, CACHE, &options()).is_err());
    let storage = MemoryStorage::new().with(PATH, Vec::new());
    assert!(MobiDocument::open_converted(&storage, PATH, CACHE, &options()).is_err());
}

#[test]
fn text_without_spaces_is_cut_in_pieces() {
    let html = format!("<p>{}</p>", "字".repeat(12_000));
    let (storage, document) = open(&Builder::new(&html));
    let all = blocks(&storage, &document);
    assert!(all.len() >= 4, "{}", all.len());
    assert!(all.iter().all(|(_, t)| t.len() <= 9000));
    assert_eq!(
        all.iter().map(|(_, t)| t.chars().count()).sum::<usize>(),
        12_000
    );
}
