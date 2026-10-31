//! Highlights and bookmarks keep their text when the book is converted
//! otherwise (its own styles switched off, say): found again by their
//! XPointers, as the position is.

use super::*;
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::annotations::{self, Annotation, Mark};
use paperpaw_sdk::document::{Block, Document, Locator};
use paperpaw_zip::testing::ZipBuilder;

/// An EPUB whose chapter is styled by the book itself: converted with
/// its styles, its text holds runs the conversion without them lacks.
fn styled_epub() -> Vec<u8> {
    let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Styled</dc:title></metadata>
  <manifest><item id="c0" href="c0.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="c0"/></spine>
</package>"#;
    let styled = r#"<span style="font-weight: bold">bold</span> and <span style="font-style: italic">italic</span>"#;
    let mut body = String::new();
    for i in 0..6 {
        body.push_str(&format!(
            "<p style=\"text-align: center\">Paragraph {i}, {styled} words, {styled} again, \
             before the words that are highlighted in it.</p>"
        ));
    }
    let page = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c0</title></head><body>{body}</body></html>"#
    );
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/c0.xhtml", page)
        .build()
}

/// The text of `block` from `start` to `end`.
fn text_of(block: &Block, start: Locator, end: Locator) -> String {
    block.text[block.text_index(start.offset)..block.text_index(end.offset)].to_string()
}

/// Some text of the book at other places in its conversion with its own
/// styles and in the one without: where it starts and ends in each, and
/// the text.
fn moved_text(storage: &Shared) -> ((Locator, Locator), (Locator, Locator), String) {
    let on = EpubDocument::open_with(storage, EPUB, true).unwrap();
    let off = EpubDocument::open_with(storage, EPUB, false).unwrap();
    for section in 0..on.sections() {
        let mut at = Locator::new(section, 0);
        while let Ok(Some(block)) = on.block_at(storage, at) {
            at = Locator::new(section, block.end.max(at.offset + 1));
            if block.text.len() < 40 || !block.text.is_ascii() {
                continue;
            }
            // Words after the styled ones.
            let (from, to) = (block.text.len() - 30, block.text.len() - 10);
            let start = Locator::new(section, block.source_offset(from));
            let end = Locator::new(section, block.source_offset(to));
            let found = |l: Locator| off.locate_xpointer(storage, &on.xpointer(storage, l)?);
            let (Some(start_off), Some(end_off)) = (found(start), found(end)) else {
                continue;
            };
            if start_off != start {
                let text = block.text[from..to].to_string();
                return ((start, end), (start_off, end_off), text);
            }
        }
    }
    panic!("no text of the book moves between its conversions");
}

/// A highlight made with the book's own styles covers the same text once
/// they are switched off, and again once they are back on: it is found
/// by its XPointer in each conversion, never left at offsets of another.
#[test]
fn a_highlight_keeps_its_text_when_the_books_styles_are_switched() {
    let epub = &styled_epub();
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    storage.write(EPUB, epub).unwrap();
    let ((start, end), (start_off, end_off), text) = moved_text(&storage);
    let highlight = Annotation {
        mark: Mark {
            kind: Kind::Highlight,
            start,
            end,
            noted: true,
        },
        percent: 1,
        created: 1_700_000_000,
        updated: 0,
        text: text.clone(),
        note: String::from("mine"),
    };
    assert!(annotations::add(&storage, EPUB, highlight).is_ok());
    let mut reading = Reading::with_book(storage.clone(), EPUB, epub);
    reading.open();
    reading.run(core_actions::BACK);
    set(&mut reading, "core.publisher-styles", "off");
    reading.open();
    let all = annotations::load(&storage, EPUB);
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].mark.start, all[0].mark.end), (start_off, end_off));
    assert_eq!(all[0].note, "mine");
    let off = EpubDocument::open_with(&storage, EPUB, false).unwrap();
    let block = off.block_at(&storage, start_off).unwrap().unwrap();
    assert_eq!(text_of(&block, start_off, end_off), text);
    // Back with the styles: where it was made.
    reading.run(core_actions::BACK);
    set(&mut reading, "core.publisher-styles", "on");
    reading.open();
    let all = annotations::load(&storage, EPUB);
    assert_eq!((all[0].mark.start, all[0].mark.end), (start, end));
}

#[path = "ended.rs"]
mod ended;
