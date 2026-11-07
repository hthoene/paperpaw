//! What was read from storage, remembered for the rest of one kernel call:
//! sizes, and small files whole. Devices that
//! draw a screen in bands (the X4) call a screen's `draw` once per band,
//! and what it reads, e.g. the recently read books on the home screen,
//! would come from the card again for every band.
//!
//! Remembered only within a call and the frame it draws, emptied then,
//! so nothing changed on the storage between calls (by another program,
//! over USB) is missed. Writes through it forget what they touch at once.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_platform::{DirEntry, Storage, StorageError};

/// Files up to this size are kept whole once read.
const SMALL_FILE: u64 = 4096;
/// Paths remembered at most.
const MAX_ENTRIES: usize = 64;

#[derive(Debug)]
struct Entry {
    path: String,
    size: Result<u64, StorageError>,
    /// The whole file, once read.
    data: Option<Vec<u8>>,
}

/// What is remembered; shared by the kernel (which empties it) and the
/// storage layer that fills it.
#[derive(Debug)]
pub(crate) struct Recall {
    entries: RefCell<Vec<Entry>>,
    /// Bytes of file data kept at most.
    budget: usize,
    /// Paths that differ only in case name the same file (see
    /// [`Storage::ignores_case`]).
    ignores_case: bool,
}

impl Recall {
    pub fn new(budget: usize, ignores_case: bool) -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            budget,
            ignores_case,
        }
    }

    /// Forgets everything, at the end of a kernel call.
    pub fn clear(&self) {
        let mut entries = self.entries.borrow_mut();
        entries.clear();
        entries.shrink_to_fit();
    }

    fn position(&self, entries: &[Entry], path: &str) -> Option<usize> {
        entries.iter().position(|e| self.same(&e.path, path))
    }

    /// The same file: so a write under another spelling of a path forgets
    /// it too, where the storage takes both for one.
    fn same(&self, a: &str, b: &str) -> bool {
        if self.ignores_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    fn kept(entries: &[Entry]) -> usize {
        entries
            .iter()
            .filter_map(|e| e.data.as_ref())
            .map(Vec::len)
            .sum()
    }

    /// Forgets `path` (written, removed).
    fn forget(&self, path: &str) {
        self.entries
            .borrow_mut()
            .retain(|e| !self.same(&e.path, path));
    }

    fn remember_size(&self, path: &str, size: Result<u64, StorageError>) {
        let mut entries = self.entries.borrow_mut();
        if entries.len() < MAX_ENTRIES && self.position(&entries, path).is_none() {
            entries.push(Entry {
                path: path.to_string(),
                size,
                data: None,
            });
        }
    }
}

/// The storage, with what is read remembered (see the module).
pub(crate) struct Recalled {
    pub inner: Box<dyn Storage>,
    pub recall: Rc<Recall>,
}

impl Recalled {
    /// The whole small file at `path`, read once and kept.
    fn small_file(&self, path: &str) -> Option<()> {
        let size = self.size(path).ok()?;
        if size > SMALL_FILE {
            return None;
        }
        {
            let entries = self.recall.entries.borrow();
            let at = self.recall.position(&entries, path)?;
            if entries[at].data.is_some() {
                return Some(());
            }
            if Recall::kept(&entries) + size as usize > self.recall.budget {
                return None;
            }
        }
        let mut data = Vec::new();
        data.try_reserve_exact(size as usize).ok()?;
        data.resize(size as usize, 0);
        let mut at = 0;
        while at < data.len() {
            let room = data.len() - at;
            match self.inner.read_at(path, at as u64, &mut data[at..]) {
                Ok(0) => break,
                Ok(n) if n <= room => at += n,
                // A backend that claims more than it was given: nothing it
                // read is trusted, and the file is not kept.
                Ok(_) | Err(_) => return None,
            }
        }
        data.truncate(at);
        let mut entries = self.recall.entries.borrow_mut();
        let i = self.recall.position(&entries, path)?;
        entries[i].data = Some(data);
        Some(())
    }
}

impl Storage for Recalled {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.small_file(path).is_some() {
            let entries = self.recall.entries.borrow();
            if let Some(data) = self
                .recall
                .position(&entries, path)
                .and_then(|i| entries[i].data.as_ref())
            {
                let start = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(data.len());
                let n = buf.len().min(data.len() - start);
                buf[..n].copy_from_slice(&data[start..start + n]);
                return Ok(n);
            }
        }
        self.inner.read_at(path, offset, buf)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        self.inner.modified(path)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        {
            let entries = self.recall.entries.borrow();
            if let Some(i) = self.recall.position(&entries, path) {
                return entries[i].size;
            }
        }
        let size = self.inner.size(path);
        self.recall.remember_size(path, size);
        size
    }

    fn exists(&self, path: &str) -> bool {
        self.size(path).is_ok()
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.recall.forget(path);
        self.inner.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.recall.forget(path);
        self.inner.append(path, data)
    }

    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.recall.forget(path);
        self.inner.write_at(path, offset, data)
    }

    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.recall.forget(path);
        self.inner.truncate(path, len)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.inner.sync()
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_from(dir, from, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.recall.forget(path);
        self.inner.remove(path)
    }

    // Moving or removing folders, or making one, may touch any path.
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.recall.clear();
        self.inner.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.recall.clear();
        self.inner.create_dir(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        // Also whatever was read: tools/library-bench ends a kernel call
        // so, removing a folder that is not there.
        self.recall.clear();
        self.inner.remove_dir_all(path)
    }

    fn ignores_case(&self) -> bool {
        self.recall.ignores_case
    }

    fn free_space(&self) -> Option<u64> {
        self.inner.free_space()
    }

    fn total_space(&self) -> Option<u64> {
        self.inner.total_space()
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.inner.local_path(path, with);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use paperpaw_sdk::MemoryStorage;

    /// A storage that counts what reaches it.
    struct Counted {
        inner: MemoryStorage,
        reads: Rc<Cell<usize>>,
        sizes: Rc<Cell<usize>>,
        ignores_case: bool,
        /// Bytes a broken backend claims to have read beyond what it did.
        overclaim: usize,
    }

    impl Storage for Counted {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.reads.set(self.reads.get() + 1);
            Ok(self.inner.read_at(path, offset, buf)? + self.overclaim)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.sizes.set(self.sizes.get() + 1);
            self.inner.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.append(path, data)
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.inner.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.inner.remove(path)
        }
        fn ignores_case(&self) -> bool {
            self.ignores_case
        }
    }

    fn storage(budget: usize) -> (Recalled, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        storage_of(budget, true)
    }

    fn storage_of(
        budget: usize,
        ignores_case: bool,
    ) -> (Recalled, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        storage_claiming(budget, ignores_case, 0)
    }

    fn storage_claiming(
        budget: usize,
        ignores_case: bool,
        overclaim: usize,
    ) -> (Recalled, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        let reads = Rc::new(Cell::new(0));
        let sizes = Rc::new(Cell::new(0));
        let inner = MemoryStorage::new()
            .with("a.txt", "hello")
            .with("big.bin", "x".repeat(SMALL_FILE as usize + 1));
        let recalled = Recalled {
            inner: Box::new(Counted {
                inner,
                reads: reads.clone(),
                sizes: sizes.clone(),
                ignores_case,
                overclaim,
            }),
            recall: Rc::new(Recall::new(budget, ignores_case)),
        };
        (recalled, reads, sizes)
    }

    fn read(storage: &dyn Storage, path: &str) -> Vec<u8> {
        let mut buf = [0u8; 8];
        let n = storage.read_at(path, 1, &mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn small_files_and_sizes_are_read_once() {
        let (s, reads, sizes) = storage(1024);
        for _ in 0..10 {
            assert_eq!(read(&s, "a.txt"), b"ello");
            assert_eq!(s.size("a.txt"), Ok(5));
            assert!(!s.exists("missing.txt"));
        }
        assert_eq!((reads.get(), sizes.get()), (1, 2));
    }

    #[test]
    fn large_files_pass_through() {
        let (s, reads, _) = storage(1 << 20);
        read(&s, "big.bin");
        read(&s, "big.bin");
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn writes_forget_and_the_call_ends_it() {
        let (s, reads, _) = storage(1024);
        read(&s, "a.txt");
        s.write("a.txt", b"howdy").unwrap();
        assert_eq!(read(&s, "a.txt"), b"owdy");
        assert_eq!(s.size("a.txt"), Ok(5));
        s.append("a.txt", b"!").unwrap();
        assert_eq!(s.size("a.txt"), Ok(6));
        assert_eq!(reads.get(), 2);
        // Another spelling of the path (FAT ignores case) forgets it too.
        s.remove("A.TXT").unwrap();
        assert_eq!(read(&s, "a.txt"), b"owdy!");
        assert_eq!(reads.get(), 3);
        s.recall.clear();
        read(&s, "a.txt");
        assert_eq!(reads.get(), 4);
    }

    /// Where names differ in case, so do files: one is not taken for the
    /// other (`a.txt` and `A.txt` on a Linux disk).
    #[test]
    fn paths_in_another_case_are_other_files_where_the_storage_says_so() {
        let (s, _, _) = storage_of(1024, false);
        s.write("A.txt", b"other").unwrap();
        assert_eq!(read(&s, "a.txt"), b"ello");
        assert_eq!(read(&s, "A.txt"), b"ther");
        assert!(!s.ignores_case());
        assert!(storage_of(1024, true).0.ignores_case());
    }

    #[test]
    fn the_budget_holds() {
        let (s, reads, _) = storage(2);
        read(&s, "a.txt");
        read(&s, "a.txt");
        assert_eq!(reads.get(), 2, "too large for the budget: read each time");
    }

    /// A backend that answers a read with more bytes than the buffer holds
    /// is not believed: nothing is kept, and every read goes to it.
    #[test]
    fn a_read_count_past_the_buffer_keeps_nothing() {
        let (s, reads, _) = storage_claiming(1024, true, 1);
        let mut buf = [0u8; 8];
        for round in 1..=2 {
            // Passed through unchanged: the caller sees the backend's claim.
            assert_eq!(s.read_at("a.txt", 1, &mut buf), Ok(5));
            // One read to try to keep it, one passed through.
            assert_eq!(reads.get(), 2 * round);
        }
        let entries = s.recall.entries.borrow();
        let at = s.recall.position(&entries, "a.txt");
        assert!(at.is_none_or(|at| entries[at].data.is_none()));
    }
}
