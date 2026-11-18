//! Shared helpers of the integration tests: EPUBs built in memory, a
//! storage that records what is written, and reading a document as a
//! reader does.

// Each test file uses some of the helpers.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::{Cell, RefCell};

use paperpaw_sdk::document::{Block, Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;
use proptest::test_runner::Config as ProptestConfig;

/// `cases` cases, or as many as `PROPTEST_CASES` says, for longer runs.
pub fn cases(cases: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(cases);
    ProptestConfig::with_cases(cases)
}

/// Where the plugins keep what they make of books.
pub const CACHE: &str = ".paperpaw/plugins/";

/// The `META-INF/container.xml` naming `opf`.
pub fn container(opf: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="{opf}" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#
    )
}

/// A package document: `items` are (id, href, media type, properties),
/// `spine` the ids in reading order.
pub fn opf(metadata: &str, items: &[(&str, &str, &str, &str)], spine: &[&str]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    );
    out.push_str(metadata);
    out.push_str("</metadata><manifest>");
    for (id, href, media, properties) in items {
        out.push_str(&format!(
            r#"<item id="{id}" href="{href}" media-type="{media}" properties="{properties}"/>"#
        ));
    }
    out.push_str("</manifest><spine>");
    for id in spine {
        out.push_str(&format!(r#"<itemref idref="{id}"/>"#));
    }
    out.push_str("</spine></package>");
    out
}

/// An XHTML chapter with `body`.
pub fn xhtml(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">\
         <head><title>t</title></head><body>{body}</body></html>"
    )
}

/// An EPUB of the files `files` (name, data) after the mimetype; the
/// others deflated, or all stored.
pub fn zip(files: &[(&str, Vec<u8>)], stored: bool) -> Vec<u8> {
    let mut builder = ZipBuilder::new().stored("mimetype", "application/epub+zip");
    for (name, data) in files {
        builder = if stored {
            builder.stored(name, data.clone())
        } else {
            builder.deflated(name, data.clone())
        };
    }
    builder.build()
}

/// A plain EPUB 3: its chapters `OEBPS/c<i>.xhtml` with the bodies
/// given, a navigation document listing each, and `extra` files.
pub fn book(bodies: &[String], extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    book_with(bodies, "", extra, false)
}

/// As [`book`], with stylesheet `css` (empty: none), stored or deflated.
pub fn book_with(bodies: &[String], css: &str, extra: &[(&str, Vec<u8>)], stored: bool) -> Vec<u8> {
    let ids: Vec<String> = (0..bodies.len()).map(|i| format!("c{i}")).collect();
    let hrefs: Vec<String> = (0..bodies.len()).map(|i| format!("c{i}.xhtml")).collect();
    let mut items: Vec<(&str, &str, &str, &str)> =
        vec![("nav", "nav.xhtml", "application/xhtml+xml", "nav")];
    for (id, href) in ids.iter().zip(&hrefs) {
        items.push((id, href, "application/xhtml+xml", ""));
    }
    if !css.is_empty() {
        items.push(("css", "style.css", "text/css", ""));
    }
    let spine: Vec<&str> = ids.iter().map(String::as_str).collect();
    let package = opf(
        "<dc:title>Generated</dc:title><dc:identifier id=\"uid\">urn:test</dc:identifier>",
        &items,
        &spine,
    );
    let mut nav = String::from(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
         <body><nav epub:type=\"toc\"><ol>",
    );
    for (i, href) in hrefs.iter().enumerate() {
        nav.push_str(&format!("<li><a href=\"{href}\">Chapter {i}</a></li>"));
    }
    nav.push_str("</ol></nav></body></html>");
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "META-INF/container.xml".into(),
            container("OEBPS/content.opf").into_bytes(),
        ),
        ("OEBPS/content.opf".into(), package.into_bytes()),
        ("OEBPS/nav.xhtml".into(), nav.into_bytes()),
    ];
    for (body, href) in bodies.iter().zip(&hrefs) {
        files.push((format!("OEBPS/{href}"), xhtml(body).into_bytes()));
    }
    if !css.is_empty() {
        files.push(("OEBPS/style.css".into(), css.as_bytes().to_vec()));
    }
    for (name, data) in extra {
        files.push(((*name).into(), data.clone()));
    }
    let files: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(name, data)| (name.as_str(), data.clone()))
        .collect();
    zip(&files, stored)
}

/// Every block of every section, read as a reader does: from each
/// section's start, each block after the one before. Fails when a block
/// makes no progress or a section has implausibly many blocks.
pub fn blocks(doc: &dyn Document, storage: &dyn Storage) -> Vec<(u32, Block)> {
    let mut out = Vec::new();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        let mut count = 0;
        while let Some(block) = doc.block_at(storage, at).expect("readable") {
            assert!(block.end > at.offset, "no progress at {at:?}");
            at = Locator::new(section, block.end);
            out.push((section, block));
            count += 1;
            assert!(count < 1_000_000, "endless section {section}");
        }
    }
    out
}

/// The words of all blocks, in order.
pub fn words(doc: &dyn Document, storage: &dyn Storage) -> Vec<String> {
    blocks(doc, storage)
        .iter()
        .flat_map(|(_, b)| {
            b.text
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The text of all blocks, a line each.
pub fn text(doc: &dyn Document, storage: &dyn Storage) -> String {
    blocks(doc, storage)
        .iter()
        .map(|(_, b)| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A storage in memory that records the paths written to, and refuses
/// to let any one file grow past `limit` bytes (counting what was asked
/// to be written, so a runaway writer shows in [`Recording::largest`]).
#[derive(Debug)]
pub struct Recording {
    pub inner: MemoryStorage,
    pub written: RefCell<Vec<String>>,
    pub limit: u64,
    largest: Cell<u64>,
}

impl Recording {
    pub fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            written: RefCell::new(Vec::new()),
            limit: u64::MAX,
            largest: Cell::new(0),
        }
    }

    pub fn limited(inner: MemoryStorage, limit: u64) -> Self {
        Self {
            limit,
            ..Self::new(inner)
        }
    }

    /// The most bytes any file was asked to hold.
    pub fn largest(&self) -> u64 {
        self.largest.get()
    }

    /// Paths written that are neither `allowed` nor in a plugin's cache.
    pub fn written_outside(&self, allowed: &[&str]) -> Vec<String> {
        self.written
            .borrow()
            .iter()
            .filter(|p| !p.starts_with(CACHE) && !allowed.contains(&p.as_str()))
            .cloned()
            .collect()
    }

    fn note(&self, path: &str, size: u64) -> Result<(), StorageError> {
        self.written.borrow_mut().push(path.into());
        self.largest.set(self.largest.get().max(size));
        if size > self.limit {
            return Err(StorageError::Io);
        }
        Ok(())
    }
}

impl Storage for Recording {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note(path, data.len() as u64)?;
        self.inner.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let size = self.inner.size(path).unwrap_or(0) + data.len() as u64;
        self.note(path, size)?;
        self.inner.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.written.borrow_mut().push(to.into());
        self.inner.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.written.borrow_mut().push(path.into());
        self.inner.create_dir(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
}
