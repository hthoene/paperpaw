//! RTF documents through the document's own reading of a converted cache:
//! metadata, blocks, styles, contents, lists, tables, pictures, and what is
//! left out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{picture, rtf};
use paperpaw_plugin_format_rtf::RtfDocument;
use paperpaw_sdk::document::{Align, BlockKind, Document, DocumentError, Locator};
use paperpaw_sdk::image::{Fit, ImageFile};
use paperpaw_sdk::{MemoryStorage, OpenOptions};

const PATH: &str = "books/test.rtf";
const CACHE: &str = ".paperpaw/plugins/format-rtf/book";

fn options() -> OpenOptions {
    OpenOptions::default().image_fit(Fit::screen(480, 800))
}

fn open_bytes(bytes: Vec<u8>) -> (MemoryStorage, RtfDocument) {
    let storage = MemoryStorage::new().with(PATH, bytes);
    let document = RtfDocument::open_converted(&storage, PATH, CACHE, &options()).unwrap();
    (storage, document)
}

fn open(body: &str) -> (MemoryStorage, RtfDocument) {
    open_bytes(rtf(body))
}

/// Every block in order: (kind, text); the blocks follow each other.
fn blocks(storage: &MemoryStorage, document: &RtfDocument) -> Vec<(BlockKind, String)> {
    let mut at = Locator::new(0, 0);
    let mut all = Vec::new();
    let mut last_end = 0;
    while let Some(block) = document.block_at(storage, at).unwrap() {
        assert!(
            block.start >= last_end && block.start < block.end,
            "{block:?}"
        );
        assert!(block.end <= document.section_len(0));
        last_end = block.end;
        at = Locator::new(0, block.end);
        all.push((block.kind, block.text));
        assert!(all.len() < 100_000);
    }
    all
}

fn texts(body: &str) -> Vec<String> {
    let (storage, document) = open(body);
    blocks(&storage, &document)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

#[test]
fn metadata_comes_from_the_head() {
    let (_, document) = open_bytes(
        b"{\\rtf1\\ansi\\ansicpg1252\\deflang1031{\\info{\\title Caf\\'e9 \\u8364?}{\\author Ann Author}\
          {\\subject Fiction}}\\pard Text\\par}"
            .to_vec(),
    );
    let m = document.metadata();
    assert_eq!(m.title.as_deref(), Some("Café €"));
    assert_eq!(m.authors, ["Ann Author"]);
    assert_eq!(m.subjects, ["Fiction"]);
    assert_eq!(m.language.as_deref(), Some("de"));
    assert_eq!(document.sections(), 1);
}

#[test]
fn a_document_with_no_title_is_named_by_its_file() {
    let (_, document) = open("Text\\par");
    assert_eq!(document.metadata().title.as_deref(), Some("test"));
}

#[test]
fn text_characters_and_styles() {
    let (storage, document) =
        open("\\pard Hello \\b bold\\b0  and \\i it\\i0 . Caf\\'e9 \\u8364? \\{x\\} a\\~b\\par");
    let block = document
        .block_at(&storage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    assert_eq!(block.text, "Hello bold and it. Café € {x} a\u{a0}b");
    let runs: Vec<_> = block
        .runs
        .iter()
        .map(|r| (r.start, r.end, r.style.bits()))
        .collect();
    assert_eq!(runs, [(6, 10, 1), (15, 17, 2)]);
}

#[test]
fn underline_strike_and_scripts_end_with_their_group() {
    let (storage, document) =
        open("\\pard A{\\ul under}{\\strike s}{\\super up}{\\sub down} B\\par");
    let block = document
        .block_at(&storage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    assert_eq!(block.text, "Aundersupdown B");
    let runs: Vec<_> = block.runs.iter().map(|r| (r.start, r.end)).collect();
    assert_eq!(runs, [(1, 6), (6, 7), (7, 9), (9, 13)]);
}

#[test]
fn unicode_with_its_fallback_and_surrogates() {
    assert_eq!(
        texts("\\pard \\u233?x \\u-10179?\\u-8704?y \\uc2\\u8364\\'80\\'80z \\uc0\\u8364 w\\par"),
        ["éx 😀y €z €w"]
    );
}

#[test]
fn another_code_page() {
    let bytes = b"{\\rtf1\\ansi\\ansicpg1251 \\'cf\\'f0\\'e8\\'e2\\'e5\\'f2\\par}".to_vec();
    let (storage, document) = open_bytes(bytes);
    assert_eq!(
        blocks(&storage, &document),
        [(BlockKind::Paragraph, "Привет".into())]
    );
}

#[test]
fn paragraphs_lines_and_alignment() {
    let (storage, document) =
        open("\\pard\\qc Middle\\par\\pard One\\line Two\\par\\pard\\qr\\qj Third\\par");
    let mut at = Locator::new(0, 0);
    let mut seen = Vec::new();
    while let Some(block) = document.block_at(&storage, at).unwrap() {
        at = Locator::new(0, block.end);
        seen.push((block.text, block.align));
    }
    assert_eq!(
        seen,
        [
            ("Middle".to_string(), Align::Center),
            ("One\nTwo".to_string(), Align::Auto),
            ("Third".to_string(), Align::Justify),
        ]
    );
}

#[test]
fn headings_by_style_and_by_outline_level() {
    let (storage, document) = open(
        "{\\stylesheet{\\s0 Normal;}{\\s1 heading 1;}{\\s2 Heading 2;}{\\*\\cs3 Char;}}\
         \\pard\\s1 Chapter One\\par\\pard Plain text\\par\\pard\\s2 Sub\\par\
         \\pard\\outlinelevel3 Deep\\par",
    );
    assert_eq!(
        blocks(&storage, &document),
        [
            (BlockKind::Heading(1), "Chapter One".into()),
            (BlockKind::Paragraph, "Plain text".into()),
            (BlockKind::Heading(2), "Sub".into()),
            (BlockKind::Heading(4), "Deep".into()),
        ]
    );
    // The contents list the first three levels.
    assert_eq!(document.toc_len(&storage), 2);
    let first = document.toc_entry(&storage, 0).unwrap();
    assert_eq!((first.title.as_str(), first.level), ("Chapter One", 0));
    let second = document.toc_entry(&storage, 1).unwrap();
    assert_eq!((second.title.as_str(), second.level), ("Sub", 1));
    assert!(second.at.offset > first.at.offset);
    assert!(document.toc_entry(&storage, 2).is_none());
}

#[test]
fn lists_with_the_writers_text_and_without() {
    assert_eq!(
        texts(
            "\\pard\\ls1\\ilvl0{\\listtext\\f1\\'b7\\tab}First\\par\
             \\pard\\ls1{\\listtext 1.\\tab}Second\\par\\pard\\ls2 Third\\par\
             \\pard Plain\\par"
        ),
        ["• First", "1. Second", "• Third", "Plain"]
    );
}

#[test]
fn a_table_is_a_line_a_row() {
    assert_eq!(
        texts(
            "\\trowd\\cellx1000\\cellx2000\\pard\\intbl A\\cell \\pard\\intbl B\\par\\cell \\row\
             \\trowd\\cellx1000\\cellx2000\\pard\\intbl\\cell \\pard\\intbl D\\cell \\row\
             \\pard After\\par"
        ),
        ["A | B", "| D", "After"]
    );
}

#[test]
fn what_is_not_known_is_left_out() {
    assert_eq!(
        texts(
            "{\\colortbl;\\red0\\green0\\blue0;}{\\*\\generator Foo}{\\info{\\title T}}\
             {\\header Header text\\par}{\\footnote Foot}{\\*\\listtable{\\list x}}\
             \\pard Visible {\\field{\\*\\fldinst HYPERLINK \"http://x\"}{\\fldrslt link}}\
             {\\*\\unknown {\\b nested} text} more{\\v hidden}{\\object{\\*\\objdata 00}}\
             \\bin3 {}}\\par After\\par"
        ),
        ["Visible link more", "After"]
    );
}

#[test]
fn deep_groups_and_odd_braces_do_no_harm() {
    let deep = "{".repeat(500);
    assert_eq!(
        texts(&format!(
            "\\pard A{deep}\\b B{}}}}} C\\par",
            "}".repeat(500)
        )),
        ["AB C"]
    );
}

#[test]
fn a_picture_is_a_block_and_decodes_when_asked() {
    let (storage, document) = open(&format!(
        "\\pard Before {}After\\par\\pard {{\\*\\shppict{}}}{{\\nonshppict{{\\pict\\wmetafile8 0102}}}}\\par",
        picture(20, 12),
        picture(8, 8),
    ));
    let all = blocks(&storage, &document);
    let kinds: Vec<_> = all
        .iter()
        .map(|(kind, text)| (*kind, text.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (BlockKind::Paragraph, "Before"),
            (BlockKind::Image, ""),
            (BlockKind::Paragraph, "After"),
            (BlockKind::Image, ""),
        ]
    );
    let mut at = Locator::new(0, 0);
    let mut images = Vec::new();
    while let Some(block) = document.block_at(&storage, at).unwrap() {
        at = Locator::new(0, block.end);
        if let Some(image) = block.image {
            images.push(image);
        }
    }
    assert_eq!(images.len(), 2);
    assert_eq!((images[0].width, images[0].height), (20, 12));
    assert!(ImageFile::open(&storage, &images[0].path).is_none());
    let ready = document.prepare_image(&storage, &images[0]).unwrap();
    let file = ImageFile::open(&storage, &ready.path).unwrap();
    assert_eq!((file.width, file.height), (20, 12));
    // The second picture is another file.
    assert_ne!(images[0].path, images[1].path);
    let second = document.prepare_image(&storage, &images[1]).unwrap();
    assert_eq!((second.width, second.height), (8, 8));
}

#[test]
fn a_damaged_picture_is_left_out() {
    let texts = texts(
        "\\pard A{\\pict\\pngblip 0102030405060708}B{\\pict\\jpegblip zz}C{\\pict\\pngblip}D\\par",
    );
    assert_eq!(texts, ["ABCD"]);
}

#[test]
fn tokens_cut_between_two_reads_are_whole() {
    // Longer than a few reads, so the tokens meet the cuts at every place.
    let paragraph = "\\pard Caf\\'e9 \\u8364?, \\b bold\\b0 \\par\r\n";
    let (storage, document) = open(&paragraph.repeat(2000));
    let all = blocks(&storage, &document);
    assert_eq!(all.len(), 2000);
    assert!(all.iter().all(|(_, text)| text == "Café €, bold"));
    // Any place in a block gives the block.
    let first = document
        .block_at(&storage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    for offset in first.start..first.end {
        let again = document
            .block_at(&storage, Locator::new(0, offset))
            .unwrap()
            .unwrap();
        assert_eq!((again.start, again.end), (first.start, first.end));
    }
}

#[test]
fn a_long_paragraph_is_cut_in_pieces_that_go_on() {
    let words = "word ".repeat(2000);
    let (storage, document) = open(&format!("\\pard {words}\\par"));
    let mut at = Locator::new(0, 0);
    let mut pieces = Vec::new();
    while let Some(block) = document.block_at(&storage, at).unwrap() {
        at = Locator::new(0, block.end);
        pieces.push((block.continued, block.continues, block.text.len()));
    }
    assert!(pieces.len() >= 3, "{pieces:?}");
    assert!(!pieces[0].0 && pieces[0].1);
    assert!(pieces.last().is_some_and(|p| p.0 && !p.1));
}

#[test]
fn what_is_no_rtf_is_refused() {
    for bytes in [&b"Plain text"[..], b"", b"{\\rt", b"{\\par}"] {
        let storage = MemoryStorage::new().with(PATH, bytes.to_vec());
        let error = RtfDocument::open_converted(&storage, PATH, CACHE, &options()).unwrap_err();
        assert!(matches!(error, DocumentError::Malformed(_)), "{error:?}");
    }
}
