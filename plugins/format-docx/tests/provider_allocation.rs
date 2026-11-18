//! Every allocation of the provider's calls is refused in turn (the test's own
//! storage is shielded): a refusal is `NoMemory`, never a panic, never a failed
//! document; the same provider, called again, answers what a call without a
//! refusal answers. The conversion is one of the calls.

#![allow(
    dead_code,
    unused_imports,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

mod native;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use native::{SIDE, call, docx, docx_with, heading, open_call, p, rich_body};
use paperpaw_plugin_format_docx::provider::DocxProvider;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

thread_local! {
    static FAIL_AT: Cell<usize> = const { Cell::new(usize::MAX) };
    /// Set while the test's own storage works: its allocations are not the provider's.
    static SHIELD: Cell<bool> = const { Cell::new(false) };
}

struct Allocator;

fn refuse() -> bool {
    if SHIELD.try_with(Cell::get).unwrap_or(true) {
        return false;
    }
    FAIL_AT
        .try_with(|count| {
            let n = count.get();
            if n == usize::MAX {
                return false;
            }
            count.set(if n == 0 { usize::MAX } else { n - 1 });
            n == 0
        })
        .unwrap_or(false)
}

// SAFETY: all successful operations and deallocations delegate unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            std::ptr::null_mut()
        } else {
            unsafe { System.realloc(ptr, layout, size) }
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

/// Runs `f` with the test's allocations shielded from the refusals.
fn shielded<T>(f: impl FnOnce() -> T) -> T {
    let before = SHIELD.replace(true);
    let out = f();
    SHIELD.set(before);
    out
}

/// Root 3 (the book, path `""`) and root 6 (any other path), shielded.
struct Roots(MemoryStorage);

impl Roots {
    fn new(book: Vec<u8>) -> Self {
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
        shielded(|| self.0.read_at(&Self::map(path), offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        shielded(|| self.0.size(&Self::map(path)))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        shielded(|| {
            if path.is_empty() {
                return Err(StorageError::Unsupported);
            }
            self.0.write(&Self::map(path), data)
        })
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        shielded(|| {
            if path.is_empty() {
                return Err(StorageError::Unsupported);
            }
            self.0.append(&Self::map(path), data)
        })
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        shielded(|| {
            if path.is_empty() {
                return Err(StorageError::Unsupported);
            }
            self.0.remove(&Self::map(path))
        })
    }
}

/// One call with the `n`-th allocation of it refused.
fn go(p: &mut DocxProvider, io: &Roots, input: &[u8], n: usize) -> Result<usize, Error> {
    let mut out = vec![0u8; 3840];
    FAIL_AT.set(n);
    let result = p.call(io, input, &mut out);
    FAIL_AT.set(usize::MAX);
    result
}

/// The whole reading: open, prepare, the metadata, sections, contents, links,
/// blocks and pictures, each call with its `n`-th allocation refused.
fn read(io: &Roots, p: &mut DocxProvider, n: usize) -> Result<(), Error> {
    let mut calls = vec![
        open_call(1),
        call(3, 1, |_| {}),
        call(5, 1, |_| {}),
        call(6, 1, |c| c.flags = 1),
        call(7, 1, |_| {}),
        call(9, 1, |_| {}),
        call(10, 1, |c| c.index = 1),
        call(14, 1, |c| c.index = u32::MAX),
        call(15, 1, |c| c.text = "#0"),
    ];
    // The blocks (a second read of each finds what the first made), then the
    // pictures, at the page's size and larger.
    for at in [0, 14, 27, 41, 55, 70, 90, 110, 140, 170, 200] {
        calls.push(call(8, 1, |c| c.offset = at));
        calls.push(call(8, 1, |c| c.offset = at));
    }
    for (index, side) in [(0, SIDE), (0, SIDE + 200), (1, SIDE), (2, SIDE)] {
        calls.push(call(18, 1, |c| {
            c.index = index;
            c.image_side = side;
        }));
    }
    for input in &calls {
        match go(p, io, input, n) {
            // Ops that have no answer here are no refusals.
            Ok(_) | Err(Error::NotFound | Error::Busy | Error::State | Error::Invalid) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(())
}

fn sweep(body: &str, rels: &str, most: usize, least: usize) {
    let data = docx_with(body, rels);
    read(
        &Roots::new(data.clone()),
        &mut DocxProvider::new(),
        usize::MAX,
    )
    .unwrap();
    let mut refused = 0;
    for n in 0..most {
        let io = Roots::new(data.clone());
        let mut p = DocxProvider::new();
        match read(&io, &mut p, n) {
            Ok(()) => {}
            Err(Error::NoMemory) => {
                refused += 1;
                let mut out = [0u8; 64];
                let _ = p.call(&io, &call(4, 1, |_| {}), &mut out);
                read(&io, &mut p, usize::MAX).unwrap_or_else(|e| panic!("refusal {n}: then {e:?}"));
                // And the reading after it is the reading without a refusal.
                let fresh = Roots::new(data.clone());
                let mut q = DocxProvider::new();
                read(&fresh, &mut q, usize::MAX).unwrap();
                assert_eq!(
                    block_texts(&io, &mut p),
                    block_texts(&fresh, &mut q),
                    "refusal {n}"
                );
            }
            Err(other) => panic!("allocation {n} refused: {other:?}"),
        }
    }
    assert!(refused > least, "{refused} refusals reached");
    // The sweep reaches past the last allocation of every call.
    read(&Roots::new(data), &mut DocxProvider::new(), most - 1)
        .unwrap_or_else(|e| panic!("{most} is too few allocations to sweep: {e:?}"));
}

/// The text of the first blocks, as a call answers them.
fn block_texts(io: &Roots, p: &mut DocxProvider) -> Vec<Vec<u8>> {
    let mut out = vec![0u8; 3840];
    let _ = p.call(io, &call(4, 1, |_| {}), &mut out);
    let _ = p.call(io, &open_call(1), &mut out);
    let _ = p.call(io, &call(3, 1, |_| {}), &mut out);
    [0u32, 14, 27, 41]
        .iter()
        .map(|&at| {
            let n = p.call(io, &call(8, 1, |c| c.offset = at), &mut out);
            match n {
                Ok(n) => out[..n].to_vec(),
                Err(e) => vec![i32::from(e) as u8],
            }
        })
        .collect()
}

#[test]
fn every_allocation_of_a_read_may_be_refused() {
    sweep(&rich_body(), "", 4000, 100);
}

/// Many headings, runs and addresses: the tables and the pages that grow.
#[test]
fn a_larger_document_may_be_refused_at_every_allocation_too() {
    let mut body = String::new();
    for n in 0..12 {
        body.push_str(&heading(1, &format!("Chapter {n}")));
        body.push_str(&p(&format!("Text of chapter {n}.")));
    }
    let mut rels = String::new();
    body.push_str("<w:p>");
    for n in 0..4 {
        rels.push_str(&format!(
            "<Relationship Id=\"u{n}\" Type=\"http://x/hyperlink\" Target=\"https://example.org/{n}\" TargetMode=\"External\"/>"
        ));
        body.push_str(&format!(
            "<w:hyperlink r:id=\"u{n}\"><w:r><w:rPr><w:b/></w:rPr><w:t>link{n} </w:t></w:r></w:hyperlink>"
        ));
    }
    body.push_str("</w:p>");
    body.push_str(&rich_body());
    sweep(&body, &rels, 6000, 100);
}
