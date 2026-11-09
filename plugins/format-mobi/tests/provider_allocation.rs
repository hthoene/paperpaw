//! Every allocation of the provider's open, prepare, meta, sections, block,
//! contents and links calls is refused in turn: a refusal is `NoMemory`, never
//! a panic and never a failed book, and the same call made again with memory
//! back gives what a call without a refusal gives. Decoding a picture (ops 17
//! and 18) reaches the SDK's image writers and is not swept: see
//! `PORTING-7.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::Builder;
use common::io::{Roots, call, open_call, rows, state};
use paperpaw_plugin_format_mobi::provider::MobiProvider;
use paperpaw_sdk::native::Error;
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

/// The storage of the test: its own allocations are the test's, not the
/// plugin's, and are never refused.
struct Quiet(Roots);

fn quiet<T>(f: impl FnOnce() -> T) -> T {
    let saved = FAIL_AT.replace(usize::MAX);
    let result = f();
    FAIL_AT.set(saved);
    result
}

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        quiet(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        quiet(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        quiet(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        quiet(|| self.0.append(path, data))
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        quiet(|| self.0.remove(path))
    }
}

const HTML: &str = "<html><body><h1>The Title</h1>\
    <p>Hello <b>bold</b> and <i>it</i>. Next &amp; last &#233; <a filepos=0000000000>link</a></p>\
    <h2>Second</h2><ul><li>first</li><li>second</li></ul>\
    <p><img recindex=\"00001\" alt=\"A grey\"/></p><p>After the picture.</p></body></html>";

fn book() -> Vec<u8> {
    let mut b = Builder::new(&HTML.replace("0000000000", &format!("{:010}", 40)));
    b.authors = vec!["Ann".into(), "Bo".into()];
    b.subjects = vec!["Fiction".into()];
    b.record_size = 256;
    // A second text record.
    b.text
        .extend(b"<p>More text to make a second record of the book, long enough. </p>".repeat(6));
    b.pictures = vec![common::png(20, 20)];
    b.build()
}

/// Which call of a read has its `n`th allocation refused (one refusal per
/// read: a preparation that starts again after a refusal would be refused
/// again at the same place by every later step, for ever), and how many calls
/// went by and were refused.
struct Sweep {
    target: usize,
    n: usize,
    calls: usize,
    refused: usize,
}

/// One call, refused where the sweep says; a refusal is answered again without
/// one, and must then succeed.
fn tried(p: &mut MobiProvider, io: &Quiet, input: &[u8], sweep: &mut Sweep) -> Vec<u8> {
    let mut out = vec![0u8; 3840];
    let n = if sweep.calls == sweep.target {
        sweep.n
    } else {
        usize::MAX
    };
    sweep.calls += 1;
    FAIL_AT.set(n);
    let result = p.call(io, input, &mut out);
    FAIL_AT.set(usize::MAX);
    match result {
        Ok(len) => {
            out.truncate(len);
            out
        }
        Err(Error::NoMemory) => {
            sweep.refused += 1;
            let len = p
                .call(io, input, &mut out)
                .unwrap_or_else(|e| panic!("allocation {n} refused, then {e:?}"));
            out.truncate(len);
            out
        }
        Err(other) => panic!("allocation {n} refused: {other:?}"),
    }
}

/// The whole read with the sweep's refusal; the replies and the sweep.
fn read(target: usize, n: usize) -> (Vec<Vec<u8>>, Sweep) {
    let mut sweep = Sweep {
        target,
        n,
        calls: 0,
        refused: 0,
    };
    let io = Quiet(Roots::new(book()));
    let mut p = MobiProvider::new();
    let mut replies = Vec::new();
    let open = tried(&mut p, &io, &open_call(1), &mut sweep);
    let (mut s, ..) = state(&open);
    while s == 1 {
        let reply = tried(&mut p, &io, &call(3, 1, |_| {}), &mut sweep);
        s = state(&reply).0;
    }
    assert_eq!(s, 0);
    for input in [
        call(5, 1, |_| {}),
        call(6, 1, |c| c.flags = 1),
        call(7, 1, |_| {}),
        call(9, 1, |_| {}),
        call(10, 1, |_| {}),
        call(14, 1, |c| c.index = u32::MAX),
        call(8, 1, |_| {}),
        call(8, 1, |c| c.offset = 45),
        call(8, 1, |c| c.offset = 100),
    ] {
        replies.push(tried(&mut p, &io, &input, &mut sweep));
    }
    (replies, sweep)
}

#[test]
fn every_allocation_of_a_read_may_be_refused() {
    let (wanted, clean) = read(usize::MAX, 0);
    assert_eq!(clean.refused, 0);
    assert!(rows(&wanted[3]).len() > 2);
    let mut refused = 0;
    // Every call of the read, every allocation of it. A refused preparation
    // starts again, so the number of calls a read takes may grow by one.
    for target in 0..clean.calls + 1 {
        for n in 0..120 {
            let (got, sweep) = read(target, n);
            assert_eq!(got, wanted, "call {target}, allocation {n}");
            refused += sweep.refused;
        }
    }
    assert!(refused > 40, "{refused} refusals reached");
}
