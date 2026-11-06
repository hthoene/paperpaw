//! A sleep that ends in a start: the X4s cut their power to sleep (or
//! enter a deep sleep that restarts the chip), so waking is a start of the
//! firmware, which plugins would hear as `core.started` alone. The
//! firmware therefore leaves a mark on the card as it goes to sleep, and
//! the next start takes it: that start ends a sleep, and tells the kernel
//! so (`KernelBuilder::after_sleep`, which sends `core.resume` too). A
//! power-up from nothing, by the cable or a battery put in, leaves no mark
//! and is no waking.

use paperpaw_platform::Storage;

/// The mark's file, in PaperPaw's folder on the card.
pub const FILE: &str = ".paperpaw/slept";

/// Marks that the device goes to sleep: written (and kept) before its
/// power goes, together with everything else still kept back. Not being
/// able to write the mark is no failure of the sleep: the wake then is a
/// start like another.
pub fn mark(storage: &dyn Storage) {
    // The folder is there once PaperPaw has run; a card without it has
    // had nothing written to it yet.
    let _ = storage.create_dir(".paperpaw");
    let _ = storage.write(FILE, b"");
    // Always: the last thing before the power goes writes back what plugins
    // saved while the sleep was prepared (answers to what they sent when
    // they heard `core.suspend`), also where the mark itself cannot be
    // written (a card without room, a read-only one).
    let _ = storage.sync();
}

/// Whether the last run went to sleep (it left the mark): the mark is
/// removed, so that only the start after that sleep finds it.
pub fn take(storage: &dyn Storage) -> bool {
    storage.exists(FILE) && storage.remove(FILE).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use paperpaw_core::sdk::{DirEntry, MemoryStorage, Storage, StorageError};

    /// A storage that cannot write and counts the syncs it is asked for.
    #[derive(Default)]
    struct Full {
        files: MemoryStorage,
        syncs: Cell<u32>,
    }

    impl Storage for Full {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.files.read_at(path, offset, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.files.size(path)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Full)
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Full)
        }
        fn sync(&self) -> Result<(), StorageError> {
            self.syncs.set(self.syncs.get() + 1);
            Ok(())
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.files.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.files.remove(path)
        }
        fn create_dir(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Full)
        }
    }

    /// The last thing before the power goes writes back what plugins saved
    /// in the sleep's last moments, also where the mark cannot be written.
    #[test]
    fn the_mark_writes_everything_back_even_where_it_cannot_be_written() {
        let storage = Full::default();
        mark(&storage);
        assert_eq!(storage.syncs.get(), 1);
        assert!(!take(&storage), "no mark where it could not be written");
    }

    #[test]
    fn a_start_after_a_sleep_finds_the_mark_once() {
        let storage = MemoryStorage::new();
        assert!(!take(&storage), "no sleep before");
        mark(&storage);
        assert!(storage.exists(FILE));
        assert!(take(&storage), "the start that ends the sleep");
        assert!(!storage.exists(FILE));
        assert!(!take(&storage), "the next one is no waking");
    }

    #[test]
    fn marking_twice_is_one_sleep() {
        let storage = MemoryStorage::new();
        mark(&storage);
        mark(&storage);
        assert!(take(&storage));
        assert!(!take(&storage));
    }
}
