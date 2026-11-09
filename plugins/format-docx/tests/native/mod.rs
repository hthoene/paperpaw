//! The harness of the provider tests: Word documents built in memory, the
//! book and the cache as one storage, and the calls.

#![allow(
    dead_code,
    clippy::unwrap_used,
    reason = "shared by the tests; not every test file uses every helper"
)]

use paperpaw_plugin_format_docx::provider::DocxProvider;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;

pub const SIDE: u32 = 800;
pub const PNG: &[u8] = include_bytes!("../../../../crates/paperpaw-image/tests/data/gray.png");

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

pub fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

pub fn heading(level: u8, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading{level}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"
    )
}

pub fn drawing(rid: &str, alt: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:docPr id=\"1\" name=\"P\" descr=\"{alt}\"/>\
         <a:graphic><a:graphicData><a:blip r:embed=\"{rid}\"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )
}

const STYLES: &str = "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
    <w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/></w:style>\
    <w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/></w:style>\
    <w:style w:type=\"paragraph\" w:styleId=\"ListBullet\"><w:name w:val=\"List Bullet\"/>\
      <w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>\
    <w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style>\
    </w:styles>";

const NUMBERING: &str = "<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
    <w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
    <w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum>\
    <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
    <w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>\
    </w:numbering>";

const RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
    <Relationship Id=\"rId1\" Type=\"http://x/image\" Target=\"media/image1.png\"/>\
    <Relationship Id=\"rId2\" Type=\"http://x/hyperlink\" Target=\"https://example.org/a?b=1&amp;c=2\" TargetMode=\"External\"/>\
    <Relationship Id=\"rId3\" Type=\"http://x/image\" Target=\"media/image2.emf\"/>\
    <Relationship Id=\"rId5\" Type=\"http://x/image\" Target=\"media/broken.png\"/>\
    </Relationships>";

const CORE: &str = "<cp:coreProperties xmlns:cp=\"c\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
    <dc:title>The   Report</dc:title><dc:creator>Ann Author</dc:creator><dc:language>de-DE</dc:language></cp:coreProperties>";

/// A picture whose data is damaged past its header.
pub fn broken_png() -> Vec<u8> {
    let mut broken = PNG.to_vec();
    let at = broken.len() - 20;
    broken[at..].fill(0xff);
    broken
}

/// A document with the body `body` and the usual parts.
pub fn docx(body: &str) -> Vec<u8> {
    docx_with(body, "")
}

/// As [`docx`], with the relationships `rels` besides the usual ones.
pub fn docx_with(body: &str, rels: &str) -> Vec<u8> {
    let rels = RELS.replace("</Relationships>", &format!("{rels}</Relationships>"));
    ZipBuilder::new()
        .deflated("[Content_Types].xml", "<Types/>")
        .deflated(
            "word/document.xml",
            format!("<?xml version=\"1.0\"?><w:document {NS}><w:body>{body}</w:body></w:document>"),
        )
        .deflated("word/styles.xml", STYLES)
        .deflated("word/numbering.xml", NUMBERING)
        .deflated("word/_rels/document.xml.rels", rels)
        .deflated("docProps/core.xml", CORE)
        .deflated("word/media/image1.png", PNG.to_vec())
        .deflated("word/media/image2.emf", vec![1, 2, 3])
        .deflated("word/media/broken.png", broken_png())
        .build()
}

/// The body of the main fixture: a heading, runs, a centred paragraph, a
/// list, a table, an external and an internal link with its bookmark, and
/// pictures (one fine, one that cannot be shown, one damaged).
pub fn rich_body() -> String {
    let mut body = String::new();
    body.push_str(&heading(1, "The Title"));
    body.push_str(
        "<w:p><w:r><w:t xml:space=\"preserve\">Hello </w:t></w:r>\
         <w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>\
         <w:r><w:t xml:space=\"preserve\"> and </w:t></w:r>\
         <w:r><w:rPr><w:i/><w:u w:val=\"single\"/></w:rPr><w:t>italic</w:t></w:r>\
         <w:r><w:t> &amp; more</w:t></w:r></w:p>",
    );
    body.push_str(
        "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:hyperlink r:id=\"rId2\"><w:r><w:t>external link</w:t></w:r></w:hyperlink>\
         <w:r><w:t> in the middle</w:t></w:r></w:p>",
    );
    body.push_str(&heading(2, "Second"));
    body.push_str(
        "<w:p><w:pPr><w:pStyle w:val=\"ListBullet\"/></w:pPr><w:r><w:t>first</w:t></w:r></w:p>\
         <w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"2\"/></w:numPr></w:pPr><w:r><w:t>numbered</w:t></w:r></w:p>",
    );
    body.push_str(
        "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>\
         <w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    body.push_str(
        "<w:p><w:hyperlink w:anchor=\"_Toc1\"><w:r><w:t>go there</w:t></w:r></w:hyperlink></w:p>",
    );
    body.push_str(&drawing("rId1", "A grey gradient"));
    body.push_str(&drawing("rId3", "Vector art"));
    body.push_str(&drawing("rId5", "Broken picture"));
    body.push_str("<w:bookmarkStart w:id=\"1\" w:name=\"_Toc1\"/>");
    body.push_str(&heading(1, "Target"));
    body.push_str(&p("After everything."));
    body
}

/// Root 3 (the book, path `""`) and root 6 (any other path).
pub struct Roots(pub MemoryStorage);

impl Roots {
    pub fn new(book: Vec<u8>) -> Self {
        Self(MemoryStorage::new().with("book", book))
    }
    fn map(path: &str) -> String {
        if path.is_empty() {
            "book".into()
        } else {
            format!("cache/{path}")
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(&Self::map(path), offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(&Self::map(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.write(&Self::map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.append(&Self::map(path), data)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.remove(&Self::map(path))
    }
}

pub fn call(op: u32, document: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, document, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

/// An open with the side the tests decode to.
pub fn open_call(document: u32) -> Vec<u8> {
    call(2, document, |c| {
        c.image_side = SIDE;
        c.cover_side = 300;
    })
}

pub fn run(p: &mut DocxProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; 3840];
    let n = p.call(io, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

pub fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// (state, status, flags, progress) of an open or a prepare.
pub fn state(reply: &[u8]) -> (u32, i32, u32, u32) {
    let stage::Record::FmtState(s) = rows(reply)[0] else {
        panic!("state")
    };
    (s.state, s.status, s.flags, s.progress_ppm)
}

/// Opens document 1 and prepares it until it is ready or failed.
pub fn open_ready(p: &mut DocxProvider, io: &dyn Storage) -> (u32, i32, u32) {
    let (mut s, mut status, mut flags, _) = state(&run(p, io, &open_call(1)).unwrap());
    let mut guard = 0;
    while s == 1 {
        (s, status, flags, _) = state(&run(p, io, &call(3, 1, |_| {})).unwrap());
        guard += 1;
        assert!(guard < 1000);
    }
    (s, status, flags)
}
