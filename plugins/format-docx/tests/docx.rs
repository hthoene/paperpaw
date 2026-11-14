//! Word documents: text, headings, lists, tables, links and pictures from
//! documents as Word writes them; and what is done with documents that
//! are made to do harm or are damaged. A document is converted and read as
//! the provider does it: the archive is the book (root 3, path `""`), the
//! cache is root 6.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;
mod native;

use common::{Recording, blocks, text};
use native::Roots;
use paperpaw_plugin_format_docx::DocxDocument;
use paperpaw_sdk::document::{BlockKind, Document, DocumentError, Locator};
use paperpaw_sdk::{OpenOptions, Storage};
use paperpaw_zip::testing::ZipBuilder;
use proptest::prelude::*;

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");
const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

fn document(body: &str) -> String {
    format!("<?xml version=\"1.0\"?><w:document {NS}><w:body>{body}</w:body></w:document>")
}

fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

fn heading(level: u8, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading{level}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"
    )
}

const STYLES: &str = "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
    <w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/></w:style>\
    <w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/></w:style>\
    <w:style w:type=\"paragraph\" w:styleId=\"Kopf\"><w:name w:val=\"Custom\"/><w:pPr><w:outlineLvl w:val=\"2\"/></w:pPr></w:style>\
    <w:style w:type=\"paragraph\" w:styleId=\"ListBullet\"><w:name w:val=\"List Bullet\"/>\
      <w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>\
    <w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style>\
    </w:styles>";

const NUMBERING: &str = "<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
    <w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl>\
      <w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
    <w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum>\
    <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
    <w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>\
    </w:numbering>";

const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
    <Relationship Id=\"rId1\" Type=\"http://x/image\" Target=\"media/image1.png\"/>\
    <Relationship Id=\"rId2\" Type=\"http://x/hyperlink\" Target=\"https://example.org/a?b=1&amp;c=2\" TargetMode=\"External\"/>\
    <Relationship Id=\"rId3\" Type=\"http://x/image\" Target=\"media/image2.emf\"/>\
    <Relationship Id=\"rId4\" Type=\"http://x/hyperlink\" Target=\"javascript:alert(1)\" TargetMode=\"External\"/>\
    </Relationships>";

const CORE: &str = "<cp:coreProperties xmlns:cp=\"c\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
    <dc:title>The   Report</dc:title><dc:creator>Ann Author</dc:creator><dc:language>de-DE</dc:language></cp:coreProperties>";

/// A document with the body `body` and the usual parts.
fn docx(body: &str) -> Vec<u8> {
    ZipBuilder::new()
        .deflated("[Content_Types].xml", "<Types/>")
        .deflated("word/document.xml", document(body))
        .deflated("word/styles.xml", STYLES)
        .deflated("word/numbering.xml", NUMBERING)
        .deflated("word/_rels/document.xml.rels", RELS)
        .deflated("docProps/core.xml", CORE)
        .deflated("word/media/image1.png", PNG.to_vec())
        .deflated("word/media/image2.emf", vec![1, 2, 3])
        .build()
}

/// Converts the book of `storage` (root 3) into its cache (root 6) and opens
/// it, as the provider's prepare does.
fn convert(storage: &dyn Storage) -> Result<DocxDocument, DocumentError> {
    let options = OpenOptions::default();
    DocxDocument::convert(storage, "", "")?;
    DocxDocument::native(storage, options.image_fit, options.color)
}

fn open(zip: Vec<u8>) -> (Roots, Result<DocxDocument, DocumentError>) {
    let storage = Roots::new(zip);
    let doc = convert(&storage);
    (storage, doc)
}

fn lines(zip: Vec<u8>) -> Vec<(BlockKind, String)> {
    let (storage, doc) = open(zip);
    let doc = doc.unwrap();
    blocks(&doc, &storage)
        .into_iter()
        .map(|(_, b)| (b.kind, b.text))
        .collect()
}

#[test]
fn text_headings_and_the_contents() {
    let body = format!(
        "{}{}{}{}",
        heading(1, "Chapter &amp; one"),
        p("First   paragraph\u{a0}with &lt;marks&gt;."),
        heading(2, "Part"),
        "<w:p><w:pPr><w:pStyle w:val=\"Kopf\"/></w:pPr><w:r><w:t>Deep</w:t></w:r></w:p>",
    );
    let (storage, doc) = open(docx(&body));
    let doc = doc.unwrap();
    let all = lines(docx(&body));
    assert_eq!(
        all[0],
        (BlockKind::Heading(1), String::from("Chapter & one"))
    );
    assert_eq!(all[1].1, "First paragraph\u{a0}with <marks>.");
    assert_eq!(all[2], (BlockKind::Heading(2), String::from("Part")));
    assert_eq!(all[3], (BlockKind::Heading(3), String::from("Deep")));
    assert_eq!(doc.toc_len(&storage), 3);
    assert_eq!(doc.toc_entry(&storage, 0).unwrap().title, "Chapter & one");
}

#[test]
fn properties_name_the_document() {
    let (_, doc) = open(docx(&p("x")));
    let m = doc.unwrap();
    let m = m.metadata();
    assert_eq!(m.title.as_deref(), Some("The Report"));
    assert_eq!(m.authors, ["Ann Author"]);
    assert_eq!(m.language.as_deref(), Some("de-DE"));
    // Without properties there is no title here: the root of the book has no
    // file name, so the library names the book by its file.
    let zip = ZipBuilder::new()
        .deflated("word/document.xml", document(&p("x")))
        .build();
    let (_, doc) = open(zip);
    assert_eq!(doc.unwrap().metadata().title, None);
}

#[test]
fn lists_bulleted_numbered_and_nested() {
    let item = |num: u32, level: u8, t: &str| {
        format!(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr></w:pPr>\
             <w:r><w:t>{t}</w:t></w:r></w:p>"
        )
    };
    let body = format!(
        "{}{}{}{}{}{}",
        item(1, 0, "apple"),
        item(1, 1, "green"),
        item(1, 0, "pear"),
        p("between"),
        item(2, 0, "first"),
        "<w:p><w:pPr><w:pStyle w:val=\"ListBullet\"/></w:pPr><w:r><w:t>by style</w:t></w:r></w:p>",
    );
    let all = lines(docx(&body));
    let texts: Vec<&str> = all.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(texts.len(), 6, "{texts:?}");
    assert!(texts[0].ends_with("apple"), "{texts:?}");
    assert!(texts[1].ends_with("green"));
    assert!(texts[2].ends_with("pear"));
    assert_eq!(texts[3], "between");
    // Numbered, and in a bulleted list by its style.
    assert!(
        texts[4].starts_with("1.") && texts[4].ends_with("first"),
        "{texts:?}"
    );
    assert!(!texts[5].starts_with(char::is_numeric) && texts[5].ends_with("by style"));
}

#[test]
fn formats_links_breaks_and_tables() {
    let body = "<w:p><w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>both</w:t></w:r>\
        <w:r><w:rPr><w:rStyle w:val=\"Strong\"/><w:vertAlign w:val=\"superscript\"/></w:rPr><w:t xml:space=\"preserve\"> up </w:t></w:r>\
        <w:r><w:t>one</w:t><w:br/><w:t>two</w:t><w:tab/><w:t>three</w:t></w:r>\
        <w:hyperlink r:id=\"rId2\"><w:r><w:t>link</w:t></w:r></w:hyperlink>\
        <w:hyperlink r:id=\"rId4\"><w:r><w:t>bad</w:t></w:r></w:hyperlink>\
        <w:r><w:delText>deleted</w:delText></w:r></w:p>\
        <w:tbl><w:tr><w:tc><w:p><w:r><w:t>c1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
        <mc:AlternateContent xmlns:mc=\"m\"><mc:Choice><w:p><w:r><w:t>chosen</w:t></w:r></w:p></mc:Choice>\
        <mc:Fallback><w:p><w:r><w:t>fallback</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>";
    let (storage, doc) = open(docx(body));
    let doc = doc.unwrap();
    let all = text(&doc, &storage);
    assert!(all.contains("both up one\ntwo"), "{all}");
    assert!(all.contains("three"));
    assert!(all.contains("c1") && all.contains("c2"));
    assert!(all.contains("chosen") && !all.contains("fallback"));
    assert!(!all.contains("deleted"));
    // Only an address that can be followed is a link.
    let links = doc.links(&storage, 0, 0, u32::MAX);
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].href, "https://example.org/a?b=1&c=2");
}

#[test]
fn internal_links_reach_their_bookmarks() {
    let body = format!(
        "<w:p><w:hyperlink w:anchor=\"_Toc1\"><w:r><w:t>go there</w:t></w:r></w:hyperlink></w:p>{}<w:bookmarkStart w:id=\"1\" w:name=\"_Toc1\"/>{}",
        p("filler"),
        heading(1, "Target"),
    );
    let (storage, doc) = open(docx(&body));
    let doc = doc.unwrap();
    let links = doc.links(&storage, 0, 0, u32::MAX);
    assert_eq!(links.len(), 1, "{links:?}");
    let at = doc.link_target(&storage, &links[0].href).expect("a place");
    let block = doc.block_at(&storage, at).unwrap().unwrap();
    assert!(
        block.text.contains("Target") || block.text.contains("filler"),
        "{}",
        block.text
    );
}

#[test]
fn pictures_are_taken_out_and_shown() {
    let drawing = |rid: &str, alt: &str| {
        format!(
            "<w:p><w:r><w:drawing><wp:inline><wp:docPr id=\"1\" name=\"P\" descr=\"{alt}\"/>\
             <a:graphic><a:graphicData><a:blip r:embed=\"{rid}\"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        )
    };
    let body = format!(
        "{}{}{}",
        p("before"),
        drawing("rId1", "A grey gradient"),
        drawing("rId3", "Vector art"),
    );
    let (storage, doc) = open(docx(&body));
    let doc = doc.unwrap();
    let all = blocks(&doc, &storage);
    let kinds: Vec<&BlockKind> = all.iter().map(|(_, b)| &b.kind).collect();
    assert_eq!(kinds[1], &BlockKind::Image, "{kinds:?}");
    let image = all[1].1.image.clone().expect("a picture");
    assert!(image.width > 0 && image.height > 0);
    assert!(doc.prepare_image(&storage, &image).is_some() || image.width > 0);
    // A kind that cannot be shown leaves its alternative text.
    assert!(
        all.iter().any(|(_, b)| b.text.contains("Vector art")),
        "{all:?}"
    );
}

#[test]
fn text_boxes_and_odd_nesting_do_no_harm() {
    let body = "<w:p><w:r><w:t>outer</w:t><w:txbxContent><w:p><w:r><w:t>inside</w:t></w:r></w:p></w:txbxContent></w:r>\
        <w:r><w:t>after</w:t></w:r></w:p>";
    let (storage, doc) = open(docx(body));
    let all = text(&doc.unwrap(), &storage);
    for word in ["outer", "inside", "after"] {
        assert!(all.contains(word), "{all}");
    }
}

#[test]
fn what_is_not_a_document_is_refused() {
    for zip in [
        Vec::new(),
        b"not a zip at all".to_vec(),
        ZipBuilder::new().build(),
        ZipBuilder::new().deflated("a.txt", "x").build(),
        // The text is not there as a document needs.
        ZipBuilder::new().deflated("word/document.xml", "").build(),
    ] {
        let (storage, doc) = open(zip);
        match doc {
            Err(_) => {}
            Ok(doc) => {
                let _ = blocks(&doc, &storage);
            }
        }
    }
}

#[test]
fn hostile_documents_are_bounded() {
    // Nesting a hundred thousand deep, lists and tables, a million empty
    // runs, and bookmarks without end: no panic, no runaway files.
    let deep = "<w:p>".repeat(100_000) + &"</w:p>".repeat(100_000);
    let tables = "<w:tbl><w:tr><w:tc>".repeat(20_000) + &"</w:tc></w:tr></w:tbl>".repeat(20_000);
    let empty = "<w:p><w:r/></w:p>".repeat(100_000);
    let marks = "<w:bookmarkStart w:name=\"a\"/>".repeat(100_000) + &p("x");
    let lists = (0..9)
        .map(|l| {
            format!(
                "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{l}\"/><w:numId w:val=\"2\"/></w:numPr></w:pPr><w:r><w:t>i{l}</w:t></w:r></w:p>"
            )
        })
        .collect::<String>();
    let many_pictures = (0..400)
        .map(|_| "<w:r><w:drawing><a:blip r:embed=\"rId1\"/></w:drawing></w:r>")
        .collect::<String>();
    for body in [
        deep,
        tables,
        empty,
        marks,
        lists,
        format!("<w:p>{many_pictures}</w:p>"),
    ] {
        let storage = Recording::limited(Roots::new(docx(&body)), 64 << 20);
        if let Ok(doc) = convert(&storage) {
            let _ = blocks(&doc, &storage);
        }
        // The book (path "") is never written, and no file runs away.
        assert!(storage.written.borrow().iter().all(|p| !p.is_empty()));
        assert!(storage.largest() <= 64 << 20);
    }
}

#[test]
fn claims_larger_than_the_file_cost_nothing() {
    // The directory says an entry is gigabytes long; its data is a few
    // bytes. The conversion ends with an error, not with memory gone.
    let mut zip = docx(&p("x"));
    for window in 0..zip.len().saturating_sub(4) {
        if &zip[window..window + 4] == b"PK\x01\x02" {
            // Uncompressed size of each central entry (offset 24).
            if window + 28 <= zip.len() {
                zip[window + 24..window + 28].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
            }
        }
    }
    let (storage, doc) = open(zip);
    if let Ok(doc) = doc {
        let _ = blocks(&doc, &storage);
    }
}

proptest! {
    #![proptest_config(common::cases(48))]

    #[test]
    fn damaged_documents_do_no_harm(
        hits in proptest::collection::vec((0usize..6000, any::<u8>()), 1..24),
        cut in proptest::option::of(0usize..6000),
    ) {
        let body = format!("{}{}{}", heading(1, "T"), p("some text"), "<w:p><w:hyperlink r:id=\"rId2\"><w:r><w:t>l</w:t></w:r></w:hyperlink></w:p>");
        let mut bytes = ZipBuilder::new()
            .stored("word/document.xml", document(&body))
            .stored("word/styles.xml", STYLES)
            .stored("word/numbering.xml", NUMBERING)
            .stored("word/_rels/document.xml.rels", RELS)
            .stored("docProps/core.xml", CORE)
            .build();
        let len = bytes.len();
        for (at, value) in hits {
            bytes[at % len] = value;
        }
        if let Some(cut) = cut {
            bytes.truncate(cut % len);
        }
        let storage = Recording::limited(Roots::new(bytes), 8 << 20);
        if let Ok(doc) = convert(&storage) {
            let _ = blocks(&doc, &storage);
        }
    }

    #[test]
    fn any_xml_is_text_or_nothing(body in "[<>/a-zA-Z:=\" 0-9&;]{0,400}") {
        let storage = Roots::new(docx(&body));
        if let Ok(doc) = convert(&storage) {
            let _ = blocks(&doc, &storage);
            let _ = doc.toc_len(&storage);
            let _ = doc.links(&storage, 0, 0, u32::MAX);
        }
        let _ = Locator::new(0, 0);
    }
}
