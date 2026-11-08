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

use paperpaw_plugin_format_images::provider::{ImagesProvider, MAX_DOCUMENTS};
use paperpaw_plugin_format_images::{CACHE, ImagesDocument};
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, OpenOptions, Storage, StorageError};

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");
const JPG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.jpg");
const GIF: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.gif");

/// Root 3 (the book's folder) and root 6 (paths that start with `cache:`).
struct Roots(MemoryStorage);

impl Roots {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let mut mem = MemoryStorage::new();
        for (name, data) in files {
            mem = mem.with(&format!("book/{name}"), data.to_vec());
        }
        Self(mem)
    }
    fn map(path: &str) -> String {
        match path.strip_prefix(CACHE) {
            Some(name) => format!("cache/{name}"),
            None => format!("book/{path}"),
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
            if !path.starts_with(CACHE) {
                return Err(StorageError::Unsupported);
            }
            self.0.write(&Self::map(path), data)
        })
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        shielded(|| {
            if !path.starts_with(CACHE) {
                return Err(StorageError::Unsupported);
            }
            self.0.append(&Self::map(path), data)
        })
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The entries are the fake's; what the plugin does with each is not.
        let entries = shielded(|| {
            let mut found: Vec<(String, u64, bool)> = Vec::new();
            let path = format!(
                "book{}",
                if dir.is_empty() { "" } else { "/" }.to_owned() + dir
            );
            self.0.list(&path, &mut |e| {
                found.push((e.name.to_owned(), e.size, e.is_dir))
            })?;
            Ok::<_, StorageError>(found)
        })?;
        for (name, size, is_dir) in &entries {
            visit(DirEntry::new(name, *is_dir, *size));
        }
        Ok(())
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

fn run(p: &mut ImagesProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
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

fn open(p: &mut ImagesProvider, io: &dyn Storage, name: &'static str) -> (u32, i32, u32) {
    let reply = run(p, io, &call(2, 1, |c| c.text = name)).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    (s.state, s.status, s.flags)
}

fn folder() -> Roots {
    Roots::new(&[
        ("1.png", PNG),
        ("2.jpg", JPG),
        ("10.gif", GIF),
        ("readme.txt", b"hello"),
    ])
}

/// The pictures here are no pictures (decoding one reaches `paperpaw-image`'s
/// writers, whose header allocation is not fallible yet: Kernel).
fn folder_of_names() -> Roots {
    Roots::new(&[
        ("a.png", b"not a picture at all"),
        ("b.png", b"nor this"),
        ("c.gif", b"GIF"),
    ])
}

fn read(io: &Roots, p: &mut ImagesProvider, n: usize) -> Result<(), Error> {
    let mut out = vec![0u8; 3840];
    for input in [
        call(2, 1, |c| c.text = "a.png"),
        call(5, 1, |_| {}),
        call(7, 1, |_| {}),
        call(8, 1, |c| c.section = 0),
        call(8, 1, |c| c.section = 0),
        call(8, 1, |c| c.section = 2),
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
    read(&folder_of_names(), &mut ImagesProvider::new(), usize::MAX).unwrap();
    let mut refused = 0;
    for n in 0..600 {
        let io = folder_of_names();
        let mut p = ImagesProvider::new();
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
