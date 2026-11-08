//! Test-only: a counting allocator that refuses the allocation a test arms,
//! and a storage whose own allocations are not the plugin's. Each allocation
//! of a call is refused in turn: one that would abort (an infallible
//! `format!`, `String::from`, `push`) ends the whole test binary; a refused
//! one must come back as `Error::NoMemory`, with the files as they were.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::string::String;
use std::vec::Vec;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

use super::Error;

pub(crate) struct Counting;

std::thread_local! {
    /// The allocation to refuse, counted from 1; 0 turns refusing off.
    static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn refuse() -> bool {
    let at = REFUSE_AT.try_with(Cell::get).unwrap_or(0);
    if at == 0 {
        return false;
    }
    COUNT
        .try_with(|count| {
            count.set(count.get() + 1);
            count.get() == at
        })
        .unwrap_or(false)
}

// SAFETY: counts (and refuses the allocation a test arms), and passes every
// other call to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's layout contract passes through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if refuse() {
            return core::ptr::null_mut();
        }
        // SAFETY: the pointer came from `System` with this layout.
        unsafe { System.realloc(p, layout, new) }
    }
}

/// Runs `run` with the `at`-th allocation refused (`usize::MAX`: none
/// refused, only counted); returns its result and how many it made.
fn armed<T>(at: usize, run: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|count| count.set(0));
    REFUSE_AT.with(|refuse| refuse.set(at));
    let result = run();
    REFUSE_AT.with(|refuse| refuse.set(0));
    (result, COUNT.with(Cell::get))
}

/// Builds test input or does the storage's work while nothing is refused or
/// counted.
pub(crate) fn fixture<T>(build: impl FnOnce() -> T) -> T {
    let at = REFUSE_AT.with(|refuse| refuse.replace(0));
    let value = build();
    REFUSE_AT.with(|refuse| refuse.set(at));
    value
}

/// A storage whose own allocations are not refused or counted.
#[derive(Debug)]
pub(crate) struct Quiet {
    inner: MemoryStorage,
    pub folds: bool,
    pub cannot_rename: bool,
}

impl Quiet {
    pub(crate) fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            folds: false,
            cannot_rename: false,
        }
    }
    pub(crate) fn get(&self, path: &str) -> Option<Vec<u8>> {
        fixture(|| self.inner.get(path))
    }
    /// Every file path, sorted.
    pub(crate) fn files(&self) -> Vec<String> {
        fixture(|| {
            let mut all = Vec::new();
            collect(&self.inner, "", &mut all);
            all.sort();
            all
        })
    }
}

fn collect(storage: &MemoryStorage, dir: &str, out: &mut Vec<String>) {
    let mut entries: Vec<(String, bool)> = Vec::new();
    let _ = storage.list(dir, &mut |e| entries.push((String::from(e.name), e.is_dir)));
    for (name, is_dir) in entries {
        let path = if dir.is_empty() {
            name
        } else {
            std::format!("{dir}/{name}")
        };
        if is_dir {
            collect(storage, &path, out);
        } else {
            out.push(path);
        }
    }
}

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.inner.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.inner.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.inner.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.inner.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The entries are collected on the storage's side; what the visitor
        // does with them is the plugin's heap, and is counted and refused.
        let mut entries: Vec<(String, bool, u64)> = Vec::new();
        fixture(|| {
            self.inner.list(dir, &mut |e| {
                entries.push((String::from(e.name), e.is_dir, e.size))
            })
        })?;
        for (name, is_dir, size) in &entries {
            visit(DirEntry::new(name, *is_dir, *size));
        }
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.inner.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if self.cannot_rename {
            return Err(StorageError::Unsupported);
        }
        fixture(|| self.inner.rename(from, to))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.inner.create_dir(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.inner.remove_dir_all(path))
    }
    fn exists(&self, path: &str) -> bool {
        fixture(|| self.inner.exists(path))
    }
    fn ignores_case(&self) -> bool {
        self.folds
    }
}

/// Refuses each allocation of `run` in turn on a fresh storage made by `make`:
/// with none refused it must be `Ok`; with one refused it must be
/// `Err(NoMemory)` or `Ok` (an allocation that is optional), and then
/// `intact` must hold of the storage.
pub(crate) fn sweep(
    name: &str,
    make: impl Fn() -> Quiet,
    run: impl Fn(&Quiet) -> Result<(), Error>,
    intact: impl Fn(&Quiet),
) {
    let storage = fixture(&make);
    let (whole, count) = armed(usize::MAX, || run(&storage));
    whole.unwrap_or_else(|error| panic!("{name}: {error:?} with nothing refused"));
    assert!(
        count > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
    for at in 1..=count {
        let storage = fixture(&make);
        let (result, _) = armed(at, || run(&storage));
        match result {
            Ok(()) | Err(Error::NoMemory) => {}
            Err(error) => panic!("{name}: refusing allocation {at}: {error:?}"),
        }
        intact(&storage);
    }
}
