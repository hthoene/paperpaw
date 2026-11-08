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
            count == REFUSE.with(Cell::get)
        })
        .unwrap_or(false)
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

/// What a reader sees of an opened document: metadata and every block.
/// Opens a document on a storage.
type Open = dyn Fn(&Host) -> Result<Box<dyn Document>, DocumentError>;

type Seen = (Option<String>, Vec<(u32, String, u32)>);

fn read(storage: &Host, open: &Open) -> Result<Seen, DocumentError> {
    let doc = open(storage)?;
    let mut blocks = Vec::new();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        while let Some(block) = doc.block_at(storage, at)? {
            let Block { text, end, .. } = block;
            assert!(end > at.offset, "no progress at {at:?}");
            at = Locator::new(section, end);
            host(|| blocks.push((section, text, end)));
        }
    }
    Ok((host(|| doc.metadata().title.clone()), blocks))
}

fn sweep(name: &str, file: &str, content: &str, open: &Open) {
    let fresh = || Host(MemoryStorage::new().with(file, content.as_bytes().to_vec()));
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
    sweep("markdown", "notes/n.md", MARKDOWN, &|storage| {
        let doc = MarkdownDocument::open(storage, "notes/n.md", true)?;
        Ok(host(|| Box::new(doc)))
    });
}

#[test]
fn a_refused_allocation_in_html_is_out_of_memory() {
    sweep("html", "web/p.html", HTML, &|storage| {
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
