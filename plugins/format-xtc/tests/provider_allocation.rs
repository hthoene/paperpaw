//! Every allocation of the provider's calls is refused in turn (the test's own
//! storage is shielded): a refusal is `NoMemory`, never a panic, never a failed
//! book; the same provider reads on once memory is back.

#![allow(
    dead_code,
    unused_imports,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_xtc::XtcDocument;
use paperpaw_plugin_format_xtc::provider::{MAX_DOCUMENTS, XtcProvider};
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

const HEADER: usize = 48;
const METADATA: usize = 256;
const CHAPTER: usize = 96;

/// A page: magic, width, height, data.
fn page(magic: &[u8; 4], width: u16, height: u16, data: &[u8]) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    out
}

/// A 1-bit page `width` by `height`, all white.
fn white(width: u16, height: u16) -> Vec<u8> {
    let stride = usize::from(width).div_ceil(8);
    page(
        b"XTG\0",
        width,
        height,
        &vec![0xff; stride * usize::from(height)],
    )
}

/// The offsets of the parts of a book built by [`book`].
#[allow(dead_code, reason = "the parts a test of damage would patch")]
struct Parts {
    metadata: usize,
    chapters: usize,
    index: usize,
}

/// An XTC book of `pages` with metadata and a chapter per page.
fn book(pages: &[Vec<u8>]) -> (Vec<u8>, Parts) {
    let count = pages.len();
    let metadata = HEADER;
    let chapters = metadata + METADATA;
    let index = chapters + CHAPTER * count;
    let data = index + 16 * count;
    let mut out = b"XTC\0".to_vec();
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&[0, 1, 0, 1]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(metadata as u64).to_le_bytes());
    out.extend_from_slice(&(index as u64).to_le_bytes());
    out.extend_from_slice(&(data as u64).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    let mut meta = vec![0u8; METADATA];
    meta[..5].copy_from_slice(b"Title");
    meta[246..248].copy_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&meta);
    for i in 0..count {
        let mut c = vec![0u8; CHAPTER];
        let name = format!("Chapter {}", i + 1);
        c[..name.len()].copy_from_slice(name.as_bytes());
        c[80..82].copy_from_slice(&(i as u16).to_le_bytes());
        out.extend_from_slice(&c);
    }
    let mut at = data as u64;
    for p in pages {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        at += p.len() as u64;
    }
    for p in pages {
        out.extend_from_slice(p);
    }
    (
        out,
        Parts {
            metadata,
            chapters,
            index,
        },
    )
}

/// Root 3 (the book, path `""`) and root 6 (any other path).
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
        shielded(|| self.0.remove(&Self::map(path)))
    }
}

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

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
fn call(op: u32, document: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, document, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn run(p: &mut XtcProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; 3840];
    let n = p.call(io, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn pages() -> Vec<Vec<u8>> {
    (0..5).map(|n| white(16 + n, 16)).collect()
}

fn open(p: &mut XtcProvider, io: &dyn Storage) -> (u32, i32, u32) {
    let reply = run(p, io, &call(2, 1, |_| {})).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    (s.state, s.status, s.flags)
}

fn read(io: &Roots, p: &mut XtcProvider, n: usize) -> Result<(), Error> {
    let mut out = vec![0u8; 3840];
    for input in [
        call(2, 1, |_| {}),
        call(5, 1, |_| {}),
        call(7, 1, |_| {}),
        call(9, 1, |_| {}),
        call(8, 1, |c| c.section = 1),
        call(17, 1, |_| {}),
        call(18, 1, |c| c.index = 2),
    ] {
        FAIL_AT.set(n);
        let result = p.call(io, &input, &mut out);
        FAIL_AT.set(usize::MAX);
        result?;
    }
    Ok(())
}

#[test]
fn every_allocation_of_a_read_may_be_refused() {
    let (data, _) = book(&pages());
    let io = Roots::new(data);
    read(&io, &mut XtcProvider::new(), usize::MAX).unwrap();
    let mut refused = 0;
    for n in 0..600 {
        let io = Roots::new(book(&pages()).0);
        let mut p = XtcProvider::new();
        match read(&io, &mut p, n) {
            Ok(()) => {}
            Err(Error::NoMemory) => {
                refused += 1;
                let _ = p.call(&io, &call(4, 1, |_| {}), &mut [0u8; 64]);
                read(&io, &mut p, usize::MAX).unwrap_or_else(|e| panic!("refusal {n}: then {e:?}"));
            }
            Err(other) => panic!("allocation {n} refused: {other:?}"),
        }
    }
    assert!(refused > 5, "{refused} refusals reached");
}
