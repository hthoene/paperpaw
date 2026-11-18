//! What converting a Word document asks of memory: it does not grow with
//! the document. The test binary's allocator counts what the running
//! thread holds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_plugin_format_docx::DocxDocument;
use paperpaw_sdk::{DirEntry, MemoryStorage, OpenOptions, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;

/// A storage that keeps only the start of the blocks made, so what the
/// storage holds does not count as what the conversion holds.
struct Short(MemoryStorage);

const KEPT: u64 = 2048;

/// The files that grow with the text.
fn grows(path: &str) -> bool {
    path.ends_with(".bin") || path.ends_with(".idx")
}

impl Storage for Short {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let keep = if grows(path) {
            &data[..data.len().min(KEPT as usize)]
        } else {
            data
        };
        self.0.write(path, keep)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if grows(path) && self.0.size(path).unwrap_or(0) >= KEPT {
            return Ok(());
        }
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn grow(by: usize) {
    LIVE.with(|live| {
        live.set(live.get() + by as isize);
        PEAK.with(|peak| peak.set(peak.get().max(live.get())));
    });
}

fn shrink(by: usize) {
    LIVE.with(|live| live.set(live.get() - by as isize));
}

// SAFETY: forwards every call to the system allocator, which does the work.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrink(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if new > layout.size() {
            grow(new - layout.size());
        } else {
            shrink(layout.size() - new);
        }
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most held at once while `work` ran, over what was held before.
fn peak_of(work: impl FnOnce()) -> usize {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    work();
    (PEAK.with(Cell::get) - base).max(0) as usize
}

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

/// A document of `paragraphs` paragraphs, a heading and a link in each
/// tenth.
fn long_document(paragraphs: usize) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..paragraphs {
        if i % 10 == 0 {
            body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Chapter {i}</w:t></w:r></w:p>"
            ));
        }
        body.push_str("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>The wind came over the hills and over the sea, </w:t></w:r><w:r><w:t>and the old house stood quiet in the grey light.</w:t></w:r></w:p>");
    }
    let styles = "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:style w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/></w:style></w:styles>";
    ZipBuilder::new()
        .deflated(
            "word/document.xml",
            format!("<w:document {NS}><w:body>{body}</w:body></w:document>"),
        )
        .deflated("word/styles.xml", styles)
        .build()
}

/// Converts the archive at `path` into the cache (the root of `storage`) and
/// opens it, as the provider does at prepare.
fn open(storage: &Short, path: &str) -> DocxDocument {
    let options = OpenOptions::default();
    DocxDocument::convert(storage, path, "").unwrap();
    DocxDocument::native(storage, options.image_fit, options.color).unwrap()
}

#[test]
fn converting_needs_the_same_memory_for_a_short_and_a_long_document() {
    let short = Short(MemoryStorage::new().with("s.docx", long_document(50)));
    let long = Short(MemoryStorage::new().with("l.docx", long_document(40_000)));
    // About 7 MB of text.
    let size = long.0.get("l.docx").unwrap().len();
    assert!(size > 20_000, "{size}");
    let small = peak_of(|| {
        let _ = open(&short, "s.docx");
    });
    let large = peak_of(|| {
        let doc = open(&long, "l.docx");
        drop(doc);
    });
    assert!(large < 160 * 1024, "{large} bytes for a long document");
    assert!(
        large < small + 64 * 1024,
        "short {small}, long {large}: it grows"
    );
}
