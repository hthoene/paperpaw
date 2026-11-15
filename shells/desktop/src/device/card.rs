//! The card of the device: a [`super::Library`] opened, which a script can
//! take out of its slot.
use std::rc::Rc;

use crate::budget;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_fs::DirStorage;
use paperpaw_platform::{DirEntry, Storage, StorageError};

/// A [`super::Library`] opened: what the kernel reads and writes.
#[derive(Debug)]
pub enum Card {
    Dir(DirStorage),
    Memory(Rc<MemoryStorage>),
}

thread_local! {
    /// The card is out of its slot ([`super::Device::card_out`]).
    static CARD_OUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Takes the card out of its slot or puts it back.
pub(super) fn set_out(out: bool) {
    CARD_OUT.set(out);
}

/// The answer of a card that is out: [`StorageError::CARD_GONE`].
fn card_in() -> Result<(), StorageError> {
    if CARD_OUT.get() {
        Err(StorageError::CARD_GONE)
    } else {
        Ok(())
    }
}

impl Card {
    /// `f` on the storage. The files of a card in memory are the card's,
    /// not the device's heap: what keeping them takes is not counted
    /// against the budget (a decoded picture would fill an X4's).
    fn on<R>(&self, f: impl FnOnce(&dyn Storage) -> R) -> R {
        match self {
            Card::Dir(storage) => f(storage),
            Card::Memory(files) => budget::untracked(|| f(&**files)),
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        card_in()?;
        self.on(|s| s.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        card_in()?;
        self.on(|s| s.size(path))
    }
    fn modified(&self, path: &str) -> Option<i64> {
        card_in().ok()?;
        self.on(|s| s.modified(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.append(path, data))
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.write_at(path, offset, data))
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.truncate(path, len))
    }
    fn sync(&self) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.sync())
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        card_in()?;
        // What the visitor keeps is the kernel's: counted.
        match self {
            Card::Dir(storage) => storage.list(dir, visit),
            Card::Memory(files) => files.list(dir, visit),
        }
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        card_in()?;
        // As `list`.
        match self {
            Card::Dir(storage) => storage.list_from(dir, from, visit),
            Card::Memory(files) => files.list_from(dir, from, visit),
        }
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.remove(path))
    }
    fn exists(&self, path: &str) -> bool {
        card_in().is_ok() && self.on(|s| s.exists(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.rename(from, to))
    }
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        card_in()?;
        self.on(|s| s.entry_identity(path))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.create_dir(path))
    }
    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        card_in()?;
        self.on(|s| s.remove_tree_step(root, keep_root, place))
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        card_in()?;
        self.on(|s| s.remove_dir_all(path))
    }
    fn ignores_case(&self) -> bool {
        self.on(|s| s.ignores_case())
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.on(|s| s.same_entry(a, b))
    }
    fn free_space(&self) -> Option<u64> {
        card_in().ok()?;
        self.on(|s| s.free_space())
    }
    fn total_space(&self) -> Option<u64> {
        card_in().ok()?;
        self.on(|s| s.total_space())
    }
    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.on(|s| s.local_path(path, with));
    }
}
