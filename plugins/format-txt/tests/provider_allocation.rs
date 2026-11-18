//! Every allocation of the provider's open, meta, sections and block calls is
//! refused in turn: a refusal is `NoMemory`, never a panic, never a failed
//! book; and the same provider reads the book once memory is back.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_format_txt::preparation::{Preparation, UTF8_CACHE};
use paperpaw_plugin_format_txt::provider::{Io, TxtProvider};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static FAIL_AT: Cell<usize> = const { Cell::new(usize::MAX) }; }
struct Allocator;
fn refuse() -> bool {
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

struct Book(Vec<u8>);

impl Storage for Book {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if !path.is_empty() {
            return Err(StorageError::NotFound);
        }
        let from = usize::try_from(offset).unwrap().min(self.0.len());
        let n = buf.len().min(self.0.len() - from);
        buf[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if path.is_empty() {
            Ok(self.0.len() as u64)
        } else {
            Err(StorageError::NotFound)
        }
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

impl Io for Book {
    fn convert(&self, _: &mut Preparation) -> Result<bool, Error> {
        Err(Error::Unsupported)
    }
}

fn call(op: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

/// The calls of a read, with the allocator failing at `n`; the first refusal
/// of each call must be `NoMemory`.
fn read(io: &Book, p: &mut TxtProvider, n: usize) -> Result<(), Error> {
    let mut out = vec![0u8; 3840];
    for input in [
        call(2, |_| {}),
        call(5, |_| {}),
        call(7, |_| {}),
        call(8, |_| {}),
        call(8, |c| c.offset = 300),
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
    let mut text = String::from(
        "Title: A Tale\nAuthor: Some One\n\n*** START OF THE PROJECT GUTENBERG EBOOK ***\n",
    );
    for n in 0..40 {
        text.push_str(&format!("Paragraph {n} with Größe and some words.\n\n"));
    }
    let io = Book(text.into_bytes());
    // Without a refusal.
    read(&io, &mut TxtProvider::new(), usize::MAX).unwrap();
    let mut refused = 0;
    for n in 0..400 {
        let mut p = TxtProvider::new();
        match read(&io, &mut p, n) {
            Ok(()) => {}
            Err(Error::NoMemory) => {
                refused += 1;
                // Memory is back: the same provider reads the book, from open on
                // when the open itself was refused (no failed document is left).
                let _ = p.call(&io, &call(4, |_| {}), &mut [0u8; 64]);
                read(&io, &mut p, usize::MAX).unwrap_or_else(|e| panic!("refusal {n}: then {e:?}"));
            }
            Err(other) => panic!("allocation {n} refused: {other:?}"),
        }
    }
    assert!(refused > 5, "{refused} refusals reached");
    let _ = UTF8_CACHE;
}
