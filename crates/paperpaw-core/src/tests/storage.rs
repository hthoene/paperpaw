//! The storage as plugins see it, through the kernel's own layers.

use super::*;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;

/// A storage that takes names in another case for the same, as FAT does.
pub(super) struct Fat(pub(super) MemoryStorage);

impl Storage for Fat {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn ignores_case(&self) -> bool {
        true
    }
}

/// Plugins learn whether names in another case are the same file from
/// the storage the shell gave, through every layer the kernel puts over
/// it.
#[test]
fn the_storage_says_whether_it_ignores_case() {
    let kernel = Kernel::builder(DEVICE)
        .storage(Fat(MemoryStorage::new()))
        .build()
        .unwrap();
    assert!(kernel.storage().ignores_case());
    assert!(kernel.shared.place(Instant(0)).storage().ignores_case());
    let kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    assert!(!kernel.storage().ignores_case());
}
