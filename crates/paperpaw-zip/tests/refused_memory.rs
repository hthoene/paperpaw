//! Memory refused while an archive is opened and read is an error
//! ([`ZipError::OutOfMemory`]), never an abort: the global allocator here
//! refuses allocations while a test thread has armed it (per thread,
//! undone on drop, so other tests never see a refusal).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;
use paperpaw_zip::{ZipArchive, ZipError};

/// This thread's refusals: allocations after `left` more are refused while
/// `armed`; `refused` counts refusals.
struct Arm {
    armed: Cell<bool>,
    left: Cell<usize>,
    refused: Cell<usize>,
}

thread_local! {
    static ARM: Arm = const {
        Arm {
            armed: Cell::new(false),
            left: Cell::new(0),
            refused: Cell::new(0),
        }
    };
}

fn allowed() -> bool {
    ARM.try_with(|arm| {
        if !arm.armed.get() {
            return true;
        }
        match arm.left.get() {
            0 => {
                arm.refused.set(arm.refused.get() + 1);
                false
            }
            left => {
                arm.left.set(left - 1);
                true
            }
        }
    })
    .unwrap_or(true)
}

struct Refusing;

// SAFETY: forwards to the system allocator, or refuses with null, which
// `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !allowed() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout, passed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by `System` above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if !allowed() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's block and layout, passed on.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Disarms on drop, also when a test panics.
struct Armed;

impl Drop for Armed {
    fn drop(&mut self) {
        ARM.with(|arm| arm.armed.set(false));
    }
}

/// Runs `f` with this thread's allocations after the first `left`
/// refused; also tells how many were refused.
fn armed_after<T>(left: usize, f: impl FnOnce() -> T) -> (T, usize) {
    ARM.with(|arm| {
        arm.refused.set(0);
        arm.left.set(left);
        arm.armed.set(true);
    });
    let out = {
        let _armed = Armed;
        f()
    };
    (out, ARM.with(|arm| arm.refused.get()))
}

/// An archive in memory; reads allocate nothing.
struct OneFile(Vec<u8>);

impl Storage for OneFile {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(self.0.len() as u64)
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

/// Opens the archive, finds two entries by name, lists all and reads each.
fn read_all(storage: &OneFile) -> Result<usize, ZipError> {
    let archive = ZipArchive::open(storage, "x")?;
    let mut total = 0;
    for name in ["a/ü.txt", "C.BIN"] {
        let entry = archive
            .entry(storage, name)?
            .ok_or(ZipError::Invalid("missing"))?;
        total += archive.read(storage, &entry, 1 << 20)?.len();
    }
    for entry in archive.entries(storage) {
        total += archive.read(storage, &entry?, 1 << 20)?.len();
    }
    Ok(total)
}

#[test]
fn every_allocation_refused_while_reading_is_an_error() {
    let text: String = (0..2000).map(|i| format!("Zeile {i}: Größe\n")).collect();
    let storage = OneFile(
        ZipBuilder::new()
            .stored("mimetype", "application/epub+zip")
            .deflated("a/ü.txt", text)
            .deflated("b.txt", "kurz")
            .stored("c.bin", vec![7u8; 300])
            .build(),
    );
    let whole = read_all(&storage).expect("reads with room");
    for left in 0.. {
        assert!(left < 10_000, "reading never ends");
        let (read, refused) = armed_after(left, || read_all(&storage));
        if refused == 0 {
            assert!(left > 5, "opening and reading allocate ({left})");
            assert_eq!(read, Ok(whole));
            break;
        }
        assert_eq!(refused, 1, "stops at the first refusal (allocation {left})");
        assert_eq!(read, Err(ZipError::OutOfMemory), "allocation {left}");
    }
}
