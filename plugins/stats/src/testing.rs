//! A storage that fails on purpose, for the tests of what happens then:
//! a card that fills up, a file that cannot be read from some point on,
//! a rename that takes the file it replaces away and then fails (as FAT
//! can), reads that return less than asked for.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

#[derive(Default)]
pub(crate) struct Faulty {
    pub inner: MemoryStorage,
    /// Writes and appends that still succeed; `None`: all of them.
    room: Cell<Option<u32>>,
    /// Files whose reads fail from an offset on.
    broken: RefCell<Vec<(String, u64)>>,
    /// Renames remove the file they replace, then fail.
    rename_drops_target: Cell<bool>,
    /// Reads return at most this many bytes.
    short_reads: Cell<Option<usize>>,
    /// Renames that still succeed; `None`: all of them. The next fails, as a
    /// power cut between two renames would end the run.
    renames: Cell<Option<u32>>,
}

impl Faulty {
    pub(crate) fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            ..Self::default()
        }
    }

    /// Takes `writes` more writes and appends, then is full.
    pub(crate) fn fill_after(&self, writes: u32) {
        self.room.set(Some(writes));
    }

    /// Reads of `path` fail from byte `at` on.
    pub(crate) fn break_at(&self, path: &str, at: u64) {
        self.broken.borrow_mut().push((path.to_string(), at));
    }

    /// Renames from now on remove the file they replace, then fail.
    pub(crate) fn drop_target_on_rename(&self) {
        self.rename_drops_target.set(true);
    }

    /// Takes `renames` more renames, then every rename fails.
    pub(crate) fn cut_after_renames(&self, renames: u32) {
        self.renames.set(Some(renames));
    }

    /// Renames work again, as after a restart.
    pub(crate) fn heal_renames(&self) {
        self.renames.set(None);
        self.rename_drops_target.set(false);
    }

    /// Reads from now on return at most `bytes` bytes.
    pub(crate) fn read_short(&self, bytes: usize) {
        self.short_reads.set(Some(bytes));
    }

    fn take_room(&self) -> Result<(), StorageError> {
        match self.room.get() {
            Some(0) => Err(StorageError::Full),
            Some(n) => {
                self.room.set(Some(n - 1));
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl Storage for Faulty {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let broken = self.broken.borrow();
        match broken.iter().find(|(p, _)| p == path) {
            Some((_, at)) if offset + buf.len() as u64 > *at => Err(StorageError::Io),
            _ => {
                let len = self
                    .short_reads
                    .get()
                    .map_or(buf.len(), |n| n.min(buf.len()));
                self.inner.read_at(path, offset, &mut buf[..len])
            }
        }
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.take_room()?;
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.take_room()?;
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        match self.renames.get() {
            Some(0) => {
                // The failing rename may have taken the file it replaces away.
                if self.rename_drops_target.get() {
                    self.inner.remove(to).ok();
                }
                return Err(StorageError::Io);
            }
            Some(n) => {
                // Within the budget a rename works.
                self.renames.set(Some(n - 1));
                return self.inner.rename(from, to);
            }
            None => {}
        }
        if self.rename_drops_target.get() {
            match self.inner.remove(to) {
                Ok(()) | Err(StorageError::NotFound) => {}
                Err(e) => return Err(e),
            }
            return Err(StorageError::Io);
        }
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }
}

/// Refuses the allocation a test arms, for the tests of a heap that says no.
/// Only one thread's allocations are counted and refused; all others pass.
pub(crate) mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub(crate) struct Refusing;

    std::thread_local! {
        /// The allocation to refuse, counted from 1; 0 turns refusing off.
        static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    /// Whether this allocation is the one to refuse (and counts it).
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

    /// Runs `run` with the `at`-th allocation refused (`usize::MAX`: none
    /// refused, only counted); its result, and how many allocations it made
    /// on this thread.
    pub(crate) fn armed<T>(at: usize, run: impl FnOnce() -> T) -> (T, usize) {
        COUNT.with(|count| count.set(0));
        REFUSE_AT.with(|refuse| refuse.set(at));
        let result = run();
        REFUSE_AT.with(|refuse| refuse.set(0));
        (result, COUNT.with(Cell::get))
    }

    /// Builds test input while nothing is refused or counted.
    pub(crate) fn fixture<T>(build: impl FnOnce() -> T) -> T {
        let at = REFUSE_AT.with(|refuse| refuse.replace(0));
        let value = build();
        REFUSE_AT.with(|refuse| refuse.set(at));
        value
    }

    // SAFETY: refuses the allocation a test arms, and passes every other call
    // to the system allocator.
    unsafe impl GlobalAlloc for Refusing {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if refuse() {
                return core::ptr::null_mut();
            }
            // SAFETY: the caller's layout, passed on.
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            // SAFETY: the pointer and layout of an allocation made above.
            unsafe { System.dealloc(p, layout) }
        }
        unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
            if refuse() {
                return core::ptr::null_mut();
            }
            // SAFETY: the pointer and layout of an allocation made above.
            unsafe { System.realloc(p, layout, new) }
        }
    }
}

use self::heap::fixture;

/// A storage whose own allocations are not the plugin's: nothing is refused
/// or counted while it works.
#[derive(Default)]
pub(crate) struct Quiet(pub MemoryStorage);

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fixture(|| self.0.list(dir, visit))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        fixture(|| self.0.rename(from, to))
    }
}

#[global_allocator]
static REFUSING: heap::Refusing = heap::Refusing;
