//! A refused allocation while a Markdown, HTML, EPUB, FictionBook or comic
//! book file is opened and read is an `OutOfMemory` answer: never an abort or a panic, never output that
//! differs from a clean run, never a cache a later try cannot repair. The
//! n-th allocation of the parser is refused, for each n.
//!
//! The only test in its binary: the allocator below is the process's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

mod common;

use paperpaw_epub_family::{
    CbzDocument, EpubDocument, Fb2Document, HtmlDocument, MarkdownDocument,
};
use paperpaw_sdk::document::{Block, Document, DocumentError, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

std::thread_local! {
    /// Allocations counted since the sweep began (not those inside storage).
    static COUNT: Cell<u64> = const { Cell::new(0) };
    /// The allocation to refuse, 1-based; 0 none.
    static REFUSE: Cell<u64> = const { Cell::new(0) };
    /// Above 0: the storage (the host's) is allocating, not the parser.
    static PAUSED: Cell<u32> = const { Cell::new(0) };
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Developer aid (`list_infallible_sites`): note where the parser
    /// allocates outside `try_reserve`.
    static SITES: std::cell::RefCell<Option<std::collections::BTreeMap<String, u64>>> =
        const { std::cell::RefCell::new(None) };
}

/// Notes the first `paperpaw` frame of an allocation that did not come
/// through a `try_*` call.
fn note_site() {
    PAUSED.with(|paused| paused.set(paused.get() + 1));
    if SITES.with(|sites| sites.borrow().is_some()) {
        let text = std::backtrace::Backtrace::force_capture().to_string();
        if !text.contains("try_reserve") && !text.contains("try_box") {
            let mut lines = text.lines();
            let mut site = None;
            while let Some(line) = lines.next() {
                if line.contains("paperpaw") && !line.contains("allocation::") {
                    let at = lines.next().unwrap_or("").trim().to_string();
                    site = Some(format!("{} {at}", line.trim()));
                    break;
                }
            }
            let site = site.unwrap_or_else(|| "(no paperpaw frame)".into());
            SITES.with(|sites| {
                *sites
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .entry(site)
                    .or_default() += 1;
            });
        }
    }
    PAUSED.with(|paused| paused.set(paused.get() - 1));
}

struct Refusing;

fn refused() -> bool {
    ARMED
        .try_with(|armed| {
            if !armed.get() || PAUSED.with(Cell::get) > 0 {
                return false;
            }
            note_site();
            let count = COUNT.with(|count| {
                count.set(count.get() + 1);
                count.get()
            });
            count == REFUSE.with(Cell::get) && !in_sdk_image()
        })
        .unwrap_or(false)
}

/// Whether the allocation asked for is the SDK's image opener's
/// (`ImageFile::open`, which copies a path with no way to refuse): its
/// follow-up is `ImageFile::try_open`, so the sweep leaves it be. The
/// decoder's own allocations are refused as any.
fn in_sdk_image() -> bool {
    PAUSED.with(|paused| paused.set(paused.get() + 1));
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let inside = backtrace.contains("paperpaw_image::ImageFile>::open")
        || backtrace.contains("paperpaw_image::Decoded>::header");
    PAUSED.with(|paused| paused.set(paused.get() - 1));
    inside
}

// SAFETY: allocations and frees are the system allocator's; a refusal is a
// null return, which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refused() {
            trace();
            return core::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refused() {
            trace();
            return core::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refused() {
            trace();
            return core::ptr::null_mut();
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Where the refusal happened, when `ALLOC_TRACE` is set (the process aborts
/// where the code cannot take it).
fn trace() {
    PAUSED.with(|paused| paused.set(paused.get() + 1));
    if std::env::var_os("ALLOC_TRACE").is_some() {
        eprintln!(
            "refused allocation {}:\n{}",
            REFUSE.with(Cell::get),
            std::backtrace::Backtrace::force_capture()
        );
    }
    PAUSED.with(|paused| paused.set(paused.get() - 1));
}

/// A storage whose own allocations are the host's, not counted or refused.
struct Host(MemoryStorage);

fn host<T>(call: impl FnOnce() -> T) -> T {
    PAUSED.with(|paused| paused.set(paused.get() + 1));
    let result = call();
    PAUSED.with(|paused| paused.set(paused.get() - 1));
    result
}

impl Storage for Host {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        host(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        host(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        host(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        host(|| self.0.append(path, data))
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        host(|| self.0.write_at(path, offset, data))
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        host(|| self.0.truncate(path, len))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        host(|| self.0.list(dir, visit))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        host(|| self.0.remove(path))
    }
    fn modified(&self, path: &str) -> Option<i64> {
        host(|| self.0.modified(path))
    }
    fn exists(&self, path: &str) -> bool {
        host(|| self.0.exists(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        host(|| self.0.rename(from, to))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        host(|| self.0.create_dir(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        host(|| self.0.remove_dir_all(path))
    }
}

/// Opens a document on a storage.
type Open = dyn Fn(&Host) -> Result<Box<dyn Document>, DocumentError>;

/// What a reader sees of an opened document: metadata and every block
/// (section, text, end, and whether it shows a picture).
type Seen = (Option<String>, Vec<(u32, String, u32, bool)>);

fn read(storage: &Host, open: &Open) -> Result<Seen, DocumentError> {
    let doc = open(storage)?;
    read_doc(&*doc, storage)
}

/// What a reader sees of `doc` already open.
fn read_doc(doc: &dyn Document, storage: &Host) -> Result<Seen, DocumentError> {
    let mut blocks = Vec::new();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        while let Some(block) = doc.block_at(storage, at)? {
            let Block {
                text, end, image, ..
            } = block;
            assert!(end > at.offset, "no progress at {at:?}");
            at = Locator::new(section, end);
            host(|| blocks.push((section, text, end, image.is_some())));
        }
    }
    probe(doc, storage)?;
    Ok((host(|| doc.metadata().title.clone()), blocks))
}

/// What a reader asks besides the text: the contents, labels, places,
/// fonts and cover. Those that cannot say "no room" answer none then
/// (not compared); the others answer `OutOfMemory` and nothing else.
fn probe(doc: &dyn Document, storage: &Host) -> Result<(), DocumentError> {
    let at = Locator::new(0, 0);
    for index in 0..doc.toc_len(storage) {
        let _ = doc.toc_entry(storage, index);
        let _ = doc.toc_target(storage, index);
    }
    let _ = doc.page_label(storage, at);
    let _ = doc.xpointer(storage, Locator::new(0, 3));
    let _ = doc.locate_xpointer(storage, "/body/DocFragment[1]/body/p[1]/text().3");
    let _ = doc.link_target(storage, "#two");
    let _ = doc.fonts(storage);
    match doc.font_data(storage, 0) {
        Ok(_) | Err(DocumentError::OutOfMemory | DocumentError::Malformed(_)) => {}
        Err(other) => panic!("font_data: {other:?}"),
    }
    match doc.cover(storage) {
        Ok(_) | Err(DocumentError::OutOfMemory) => {}
        Err(other) => panic!("cover: {other:?}"),
    }
    Ok(())
}

fn sweep(
    name: &str,
    file: &str,
    content: &[u8],
    extra: &[(&str, &[u8])],
    short_ok: bool,
    open: &Open,
) -> u32 {
    let fresh = || {
        let mut storage = MemoryStorage::new().with(file, content.to_vec());
        for (path, bytes) in extra {
            storage = storage.with(path, bytes.to_vec());
        }
        Host(storage)
    };
    // A clean run, counting what the parser allocates.
    ARMED.with(|armed| armed.set(true));
    REFUSE.with(|refuse| refuse.set(0));
    COUNT.with(|count| count.set(0));
    let clean = read(&fresh(), open).expect("a clean run reads");
    let total = COUNT.with(Cell::get);
    ARMED.with(|armed| armed.set(false));
    assert!(!clean.1.is_empty(), "{name}: the document has blocks");
    assert!(
        total > 50,
        "{name}: {total} allocations is not representative"
    );
    let mut refusals = 0;
    let mut short = 0;
    for nth in 1..=total {
        let storage = fresh();
        COUNT.with(|count| count.set(0));
        REFUSE.with(|refuse| refuse.set(nth));
        ARMED.with(|armed| armed.set(true));
        let result = read(&storage, open);
        ARMED.with(|armed| armed.set(false));
        match result {
            Err(DocumentError::OutOfMemory) => refusals += 1,
            Err(other) => panic!("{name}: allocation {nth} refused: {other:?}"),
            // A picture that found no room to be decoded is its alternative
            // text, and the chapter is not complete in the cache.
            Ok(seen) if short_ok && seen != clean && pictures_left_out(&seen, &clean) => {
                assert_eq!(
                    complete_chapters(&storage),
                    0,
                    "{name}: allocation {nth} refused: a chapter without its picture is complete"
                );
                // Short memory is not the picture's fault: it is never
                // marked undecodable (a comic has no chapters to check).
                assert_eq!(
                    cache_files_ending(&storage, ".bad"),
                    0,
                    "{name}: allocation {nth} refused: a picture is marked undecodable"
                );
                short += 1;
            }
            Ok(seen) => assert_eq!(seen, clean, "{name}: allocation {nth} refused"),
        }
        // Nothing half-made stays: the same storage, nothing refused, reads
        // as a clean run does.
        assert_eq!(
            read(&storage, open).expect("a retry reads"),
            clean,
            "{name}: retry after allocation {nth}"
        );
    }
    assert!(refusals > 0, "{name}: no refusal answered OutOfMemory");
    short
}

const MARKDOWN: &str = "# A title\n\nA paragraph with *emphasis*, **strong**, `code` and a \
[link](https://example.org/x).\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> a quote\n\n\
```\ncode block\n```\n\n---\n\n## Second heading\n\nMore text, a line\nthat goes on.\n";

const HTML: &str = "<html><head><title>Page</title><style>p.k { font-style: italic }</style>\
</head><body><h1 id=\"top\">Head</h1><p class=\"k\">A <b>bold</b> and <i>italic</i> word, a \
<a href=\"#top\">link</a>.</p><ul><li>one</li><li>two</li></ul><table><tr><td>a</td><td>b</td>\
</tr></table><h2>Next</h2><p>The last paragraph.</p></body></html>";

#[test]
fn a_refused_allocation_in_markdown_is_out_of_memory() {
    sweep(
        "markdown",
        "notes/n.md",
        MARKDOWN.as_bytes(),
        &[],
        false,
        &|storage| {
            let doc = MarkdownDocument::open(storage, "notes/n.md", true)?;
            Ok(host(|| Box::new(doc)))
        },
    );
}

#[test]
fn a_refused_allocation_in_html_is_out_of_memory() {
    sweep(
        "html",
        "web/p.html",
        HTML.as_bytes(),
        &[],
        false,
        &|storage| {
            let doc = HtmlDocument::open(storage, "web/p.html", true)?;
            Ok(host(|| Box::new(doc)))
        },
    );
}

/// The files the developer aid reads: path, bytes and how to open.
fn sites_cases() -> Vec<(&'static str, Vec<u8>, Box<Open>)> {
    vec![
        (
            "notes/n.md",
            MARKDOWN.as_bytes().to_vec(),
            Box::new(|s: &Host| {
                let doc = MarkdownDocument::open(s, "notes/n.md", true)?;
                Ok(host(|| Box::new(doc) as Box<dyn Document>))
            }),
        ),
        (
            "web/p.html",
            HTML.as_bytes().to_vec(),
            Box::new(|s: &Host| {
                let doc = HtmlDocument::open(s, "web/p.html", true)?;
                Ok(host(|| Box::new(doc) as Box<dyn Document>))
            }),
        ),
        ("book.epub", epub_fixture(), Box::new(open_epub)),
        ("b/book.fb2", fb2_fixture().into_bytes(), Box::new(open_fb2)),
        ("c.cbz", comic_fixture(), Box::new(open_comic)),
    ]
}

/// Lists where the parser allocates infallibly (run with `--ignored
/// --nocapture`): what is left to make fallible.
#[test]
#[ignore = "developer aid: lists the parser's infallible allocation sites"]
fn list_infallible_sites() {
    SITES.with(|sites| *sites.borrow_mut() = Some(Default::default()));
    for (file, content, open) in sites_cases() {
        let storage = Host(MemoryStorage::new().with(file, content));
        ARMED.with(|armed| armed.set(true));
        let _ = read(&storage, &*open);
        ARMED.with(|armed| armed.set(false));
    }
    let sites = SITES.with(|sites| sites.borrow_mut().take().unwrap());
    for (site, count) in &sites {
        eprintln!("{count:5} {site}");
    }
    eprintln!("{} sites", sites.len());
}

/// Whether `seen` is `clean` but for pictures that are their alternative
/// text instead (the text of a block that lost its picture may differ: a
/// page is named).
fn pictures_left_out(seen: &Seen, clean: &Seen) -> bool {
    seen.0 == clean.0
        && seen.1.len() == clean.1.len()
        && seen
            .1
            .iter()
            .zip(&clean.1)
            .all(|(a, b)| a == b || (!a.3 && b.3 && a.0 == b.0 && a.2 == b.2))
}

/// How many files of the cache of `storage` have names ending in `suffix`.
fn cache_files_ending(storage: &Host, suffix: &str) -> usize {
    fn walk(storage: &Host, dir: &str, suffix: &str) -> usize {
        let mut found = Vec::new();
        let _ = storage.list(dir, &mut |entry| {
            found.push((entry.name.to_string(), entry.is_dir));
        });
        found
            .iter()
            .map(|(name, is_dir)| {
                let path = if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                };
                if *is_dir {
                    walk(storage, &path, suffix)
                } else {
                    usize::from(name.ends_with(suffix))
                }
            })
            .sum()
    }
    walk(storage, "", suffix)
}

/// How many chapters the cache of `storage` holds marked complete.
fn complete_chapters(storage: &Host) -> usize {
    fn walk(storage: &Host, dir: &str) -> usize {
        let mut found = Vec::new();
        let _ = storage.list(dir, &mut |entry| {
            found.push((entry.name.to_string(), entry.is_dir));
        });
        found
            .iter()
            .map(|(name, is_dir)| {
                let path = if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                };
                if *is_dir {
                    walk(storage, &path)
                } else {
                    usize::from(name == "end")
                }
            })
            .sum()
    }
    walk(storage, "")
}

/// A 4 by 4 gray picture.
const PNG: [u8; 83] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00, 0x8c, 0x9a, 0xc1,
    0xa2, 0x00, 0x00, 0x00, 0x1a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0xb0, 0xa9, 0xd8,
    0xc2, 0x20, 0x12, 0xd0, 0x73, 0x82, 0x41, 0x23, 0x65, 0xc1, 0x1d, 0x10, 0xef, 0x03, 0x00, 0x3e,
    0x94, 0x07, 0x81, 0x87, 0x5a, 0xf1, 0x08, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// A picture that finds no room fails the chapter, whose cache is then not
/// complete: the retry shows the picture (`read` compares what each block
/// shows, a picture included, with the clean run's).
#[test]
fn a_refused_allocation_never_caches_a_chapter_without_its_picture() {
    const PAGE: &str = "# Pictures\n\nA line.\n\n![A cat](cat.png)\n\nAfter it.\n";
    let storage = Host(
        MemoryStorage::new()
            .with("notes/n.md", PAGE.as_bytes().to_vec())
            .with("notes/cat.png", PNG.to_vec()),
    );
    let (_, blocks) = read(&storage, &|s| {
        let doc = MarkdownDocument::open(s, "notes/n.md", true)?;
        Ok(host(|| Box::new(doc)))
    })
    .expect("a clean run reads");
    assert!(
        blocks.iter().any(|b| b.3),
        "the picture is shown: {blocks:?}"
    );
    let short = sweep(
        "markdown with a picture",
        "notes/n.md",
        PAGE.as_bytes(),
        &[("notes/cat.png", &PNG)],
        true,
        &|storage| {
            let doc = MarkdownDocument::open(storage, "notes/n.md", true)?;
            Ok(host(|| Box::new(doc)))
        },
    );
    assert!(short > 0, "no picture was left to its alternative text");
}

/// An EPUB of two chapters with a link, a style, a list, a table, a
/// picture and a note: opened (package, navigation, spine) and read whole.
fn epub_fixture() -> Vec<u8> {
    let chapters = [
        "<h1 id=\"top\">One</h1><p style=\"text-align:center\">A <b>bold</b> and <i>italic</i> \
         word with a <a href=\"c1.xhtml#two\">link</a>.</p><ul><li>a</li><li>b</li></ul>\
         <table><tr><td>x</td><td>y</td></tr></table><img src=\"pic.png\" alt=\"A picture\"/>"
            .to_string(),
        "<h2 id=\"two\">Two</h2><blockquote>Quoted text.</blockquote><p>The end.</p>".to_string(),
    ];
    common::book_with(
        &chapters,
        "p { margin-top: 1em } b { color: #333 }",
        &[("OEBPS/pic.png", PNG.to_vec())],
        false,
    )
}

fn open_epub(storage: &Host) -> Result<Box<dyn Document>, DocumentError> {
    let doc = EpubDocument::open(storage, "book.epub")?;
    Ok(host(|| Box::new(doc)))
}

#[test]
fn a_refused_allocation_in_an_epub_is_out_of_memory() {
    sweep("epub", "book.epub", &epub_fixture(), &[], true, &open_epub);
}

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
            out.push(if i <= chunk.len() {
                char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

/// A FictionBook with a title, an author, two sections, a note, a link and
/// a picture.
fn fb2_fixture() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FictionBook \
         xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" xmlns:l=\"http://www.w3.org/1999/xlink\">\
         <description><title-info><book-title>Story</book-title><author><first-name>Ann</first-name>\
         <last-name>Writer</last-name></author><lang>en</lang></title-info></description>\
         <body><title><p>Story</p></title><section><title><p>One</p></title>\
         <p>Text with <emphasis>emphasis</emphasis> and a <a l:href=\"#n1\">note</a>.</p>\
         <image l:href=\"#pic\"/><p>After it.</p></section>\
         <section><title><p>Two</p></title><p>More text.</p></section></body>\
         <body name=\"notes\"><section id=\"n1\"><p>The note.</p></section></body>\
         <binary id=\"pic\" content-type=\"image/png\">{}</binary></FictionBook>",
        base64(&PNG)
    )
}

fn open_fb2(storage: &Host) -> Result<Box<dyn Document>, DocumentError> {
    let doc = Fb2Document::open(storage, "b/book.fb2", true)?;
    Ok(host(|| Box::new(doc)))
}

#[test]
fn a_refused_allocation_in_a_fictionbook_is_out_of_memory() {
    sweep(
        "fictionbook",
        "b/book.fb2",
        fb2_fixture().as_bytes(),
        &[],
        true,
        &open_fb2,
    );
}

/// A comic of three pages.
fn comic_fixture() -> Vec<u8> {
    let mut builder = paperpaw_zip::testing::ZipBuilder::new();
    for name in ["p1.png", "p2.png", "p10.png"] {
        builder = builder.stored(name, PNG.to_vec());
    }
    builder.build()
}

fn open_comic(storage: &Host) -> Result<Box<dyn Document>, DocumentError> {
    let doc = CbzDocument::open(storage, "c.cbz")?;
    Ok(host(|| Box::new(doc)))
}

#[test]
fn a_refused_allocation_in_a_comic_is_out_of_memory() {
    // A comic caches no chapter, so `complete_chapters` is 0 by nature:
    // what is checked is that pictures do fall back to their text and are
    // never marked undecodable.
    let short = sweep("comic", "c.cbz", &comic_fixture(), &[], true, &open_comic);
    assert!(short > 0, "no picture was left to its alternative text");
}

/// The files named `name` of the cache of `storage`.
fn cache_files(storage: &Host, name: &str) -> Vec<String> {
    fn walk(storage: &Host, dir: &str, name: &str, out: &mut Vec<String>) {
        let mut found = Vec::new();
        let _ = storage.list(dir, &mut |entry| {
            found.push((entry.name.to_string(), entry.is_dir));
        });
        for (entry, is_dir) in found {
            let path = if dir.is_empty() {
                entry.clone()
            } else {
                format!("{dir}/{entry}")
            };
            if is_dir {
                walk(storage, &path, name, out);
            } else if entry == name {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(storage, "", name, &mut out);
    out
}

/// A book that converts a chapter again (its text gone from the cache; the
/// book keeps the chapter it read last, so it has two)
/// and finds no room part of the way must not take what that left for the
/// chapter it once had: reading on gives what a clean run gives.
#[test]
fn a_refused_reconversion_is_never_taken_for_the_chapter_the_book_had() {
    let bodies = [
        "<h1>One</h1><p>A paragraph of the chapter with <b>bold</b> and a \
         <a href=\"c0.xhtml\">link</a>.</p><p>Another paragraph, as long.</p>\
         <img src=\"pic.png\" alt=\"A picture\"/>"
            .to_string(),
        "<h2>Two</h2><p>The second chapter, so that the book has left the first.</p>".to_string(),
    ];
    let epub = common::book(&bodies, &[("OEBPS/pic.png", PNG.to_vec())]);
    let storage = Host(MemoryStorage::new().with("book.epub", epub));
    let doc = open_epub(&storage).expect("a clean open");
    let clean = read_doc(&*doc, &storage).expect("a clean run reads");
    let converted_again = || {
        for path in cache_files(&storage, "txt") {
            let _ = storage.remove(&path);
        }
    };
    // What one reconversion allocates.
    converted_again();
    ARMED.with(|armed| armed.set(true));
    REFUSE.with(|refuse| refuse.set(0));
    COUNT.with(|count| count.set(0));
    assert_eq!(read_doc(&*doc, &storage).expect("reads again"), clean);
    let total = COUNT.with(Cell::get);
    ARMED.with(|armed| armed.set(false));
    assert!(total > 20, "{total} allocations is not representative");
    let mut refusals = 0;
    for nth in 1..=total {
        converted_again();
        COUNT.with(|count| count.set(0));
        REFUSE.with(|refuse| refuse.set(nth));
        ARMED.with(|armed| armed.set(true));
        let result = read_doc(&*doc, &storage);
        ARMED.with(|armed| armed.set(false));
        match result {
            Err(DocumentError::OutOfMemory) => refusals += 1,
            Err(other) => panic!("allocation {nth} refused: {other:?}"),
            Ok(seen) => assert_eq!(seen, clean, "allocation {nth} refused"),
        }
        // The same book, the chapter's cache as the refusal left it.
        assert_eq!(
            read_doc(&*doc, &storage).expect("reads on"),
            clean,
            "reading on after allocation {nth}"
        );
        // And converted whole: both chapters are marked complete again.
        assert_eq!(
            cache_files(&storage, "end").len(),
            2,
            "a chapter not complete after allocation {nth}"
        );
    }
    assert!(refusals > 0, "no refusal answered OutOfMemory");
}
