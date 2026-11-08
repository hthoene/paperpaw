//! A refused allocation while a Markdown or HTML file is opened and read
//! is an `OutOfMemory` answer: never an abort or a panic, never output that
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

use paperpaw_plugin_format_epub::{HtmlDocument, MarkdownDocument};
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
    Ok((host(|| doc.metadata().title.clone()), blocks))
}

fn sweep(
    name: &str,
    file: &str,
    content: &str,
    extra: &[(&str, &[u8])],
    short_ok: bool,
    open: &Open,
) {
    let fresh = || {
        let mut storage = MemoryStorage::new().with(file, content.as_bytes().to_vec());
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
            Ok(seen)
                if short_ok && seen != clean && lost_pictures(&seen) == lost_pictures(&clean) =>
            {
                assert_eq!(
                    complete_chapters(&storage),
                    0,
                    "{name}: allocation {nth} refused: a chapter without its picture is complete"
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
    if short_ok {
        assert!(
            short > 0,
            "{name}: no picture was left to its alternative text"
        );
    }
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
    sweep("markdown", "notes/n.md", MARKDOWN, &[], false, &|storage| {
        let doc = MarkdownDocument::open(storage, "notes/n.md", true)?;
        Ok(host(|| Box::new(doc)))
    });
}

#[test]
fn a_refused_allocation_in_html_is_out_of_memory() {
    sweep("html", "web/p.html", HTML, &[], false, &|storage| {
        let doc = HtmlDocument::open(storage, "web/p.html", true)?;
        Ok(host(|| Box::new(doc)))
    });
}

/// Lists where the parser allocates infallibly (run with `--ignored
/// --nocapture`): what is left to make fallible.
#[test]
#[ignore = "developer aid: lists the parser's infallible allocation sites"]
fn list_infallible_sites() {
    SITES.with(|sites| *sites.borrow_mut() = Some(Default::default()));
    ARMED.with(|armed| armed.set(true));
    let storage = Host(MemoryStorage::new().with("notes/n.md", MARKDOWN.as_bytes().to_vec()));
    let _ = read(&storage, &|s| {
        let doc = MarkdownDocument::open(s, "notes/n.md", true)?;
        Ok(host(|| Box::new(doc)))
    });
    let storage = Host(MemoryStorage::new().with("web/p.html", HTML.as_bytes().to_vec()));
    let _ = read(&storage, &|s| {
        let doc = HtmlDocument::open(s, "web/p.html", true)?;
        Ok(host(|| Box::new(doc)))
    });
    ARMED.with(|armed| armed.set(false));
    let sites = SITES.with(|sites| sites.borrow_mut().take().unwrap());
    for (site, count) in &sites {
        eprintln!("{count:5} {site}");
    }
    eprintln!("{} sites", sites.len());
}

/// What a reader sees but for which blocks show a picture.
fn lost_pictures(seen: &Seen) -> Vec<(u32, &str, u32)> {
    seen.1
        .iter()
        .map(|(section, text, end, _)| (*section, text.as_str(), *end))
        .collect()
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
                    usize::from(name.ends_with(".end"))
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
    sweep(
        "markdown with a picture",
        "notes/n.md",
        PAGE,
        &[("notes/cat.png", &PNG)],
        true,
        &|storage| {
            let doc = MarkdownDocument::open(storage, "notes/n.md", true)?;
            Ok(host(|| Box::new(doc)))
        },
    );
}
