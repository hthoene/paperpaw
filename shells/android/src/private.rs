//! The passwords and keys (`paperpaw_sdk::secrets::FILE`) in the app's
//! private folder, apart from its other data.
//!
//! PaperPaw's data lives in the app's folder on the shared storage
//! (`Android/data/...`), where it can be copied off by hand; up to
//! Android 9 any app allowed to read the shared storage reads it as well.
//! The secrets file alone goes to the private folder, which only PaperPaw
//! reads; one kept in the shared folder before is moved there at start.

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::secrets::FILE;

/// The secrets file's name in the private folder.
const NAME: &str = "secrets.txt";
/// The most read when moving the file in (it holds a few short lines).
const MOST: usize = 1024 * 1024;

/// `base` with [`FILE`] kept in `private` instead.
#[derive(Debug)]
pub struct Private<B, P> {
    base: B,
    private: P,
}

impl<B: Storage, P: Storage> Private<B, P> {
    /// Keeps the secrets in `private`, moving a file kept in `base` there.
    pub fn new(base: B, private: P) -> Self {
        let storage = Self { base, private };
        storage.move_in();
        storage
    }

    fn move_in(&self) {
        if !self.base.exists(FILE) {
            return;
        }
        if self.private.exists(NAME) {
            log::warn!("{FILE}: also in the private folder, which counts");
            return;
        }
        let moved = paperpaw_sdk::read_file(&self.base, FILE, MOST)
            .and_then(|data| self.private.write(NAME, &data))
            .and_then(|()| self.base.remove(FILE));
        if let Err(e) = moved {
            log::warn!("{FILE}: not moved to the private folder: {e}");
        }
    }

    /// The storage and the path in it for `path`.
    fn route<'p>(&self, path: &'p str) -> (&dyn Storage, &'p str) {
        if path == FILE {
            (&self.private, NAME)
        } else {
            (&self.base, path)
        }
    }
}

impl<B: Storage, P: Storage> Storage for Private<B, P> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let (storage, path) = self.route(path);
        storage.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (storage, path) = self.route(path);
        storage.size(path)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let (storage, path) = self.route(path);
        storage.modified(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.append(path, data)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.private.sync().and(self.base.sync())
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.base.list(dir, visit)
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.base.list_from(dir, from, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.remove(path)
    }

    fn exists(&self, path: &str) -> bool {
        let (storage, path) = self.route(path);
        storage.exists(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        match (from == FILE, to == FILE) {
            (false, false) => self.base.rename(from, to),
            (true, true) => Ok(()),
            // Into or out of the private folder: not moved by name.
            _ => Err(StorageError::Unsupported),
        }
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.base.create_dir(path)
    }

    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        let step = self.base.remove_tree_step(root, keep_root, place)?;
        // The secrets go with the folder that holds them.
        let inside = root.is_empty()
            || FILE
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'));
        if step.done && inside {
            self.private.remove(NAME)?;
        }
        Ok(step)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.base.remove_dir_all(path)?;
        // The secrets go with the folder that holds them.
        let inside = path.is_empty()
            || FILE
                .strip_prefix(path)
                .is_some_and(|rest| rest.starts_with('/'));
        if inside {
            self.private.remove(NAME)?;
        }
        Ok(())
    }

    /// As the storage of everything but the secrets, where books are.
    fn ignores_case(&self) -> bool {
        self.base.ignores_case()
    }

    fn free_space(&self) -> Option<u64> {
        self.base.free_space()
    }

    fn total_space(&self) -> Option<u64> {
        self.base.total_space()
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        let (storage, path) = self.route(path);
        storage.local_path(path, with);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{MemoryStorage, read_file};

    /// Shared storage, which takes names in another case for the same.
    struct Shared(MemoryStorage);

    impl Storage for Shared {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(path, at, buf)
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

    /// Whether names in another case are the same file is the shared
    /// storage's, where books are.
    #[test]
    fn it_ignores_case_where_the_books_storage_does() {
        let storage = Private::new(Shared(MemoryStorage::new()), MemoryStorage::new());
        assert!(storage.ignores_case());
        let storage = Private::new(MemoryStorage::new(), Shared(MemoryStorage::new()));
        assert!(!storage.ignores_case());
    }

    #[test]
    fn secrets_are_kept_apart() {
        let storage = Private::new(MemoryStorage::new(), MemoryStorage::new());
        storage.write(FILE, b"wifi.home\tpassword").unwrap();
        storage
            .write(".paperpaw/plugins/reader/a.txt", b"a")
            .unwrap();
        assert_eq!(
            read_file(&storage, FILE, 64).unwrap(),
            b"wifi.home\tpassword"
        );
        assert_eq!(storage.base.get(FILE), None);
        assert_eq!(storage.private.get(NAME).unwrap(), b"wifi.home\tpassword");
        assert!(storage.base.exists(".paperpaw/plugins/reader/a.txt"));
        storage.remove(FILE).unwrap();
        assert!(!storage.exists(FILE));
    }

    #[test]
    fn a_shared_file_moves_in() {
        let base = MemoryStorage::new()
            .with(FILE, "translate.key\tabc")
            .with("paperpaw.ini", "");
        let storage = Private::new(base, MemoryStorage::new());
        assert_eq!(storage.base.get(FILE), None);
        assert_eq!(
            read_file(&storage, FILE, 64).unwrap(),
            b"translate.key\tabc"
        );
        // A private one counts; the other stays where it is.
        let base = MemoryStorage::new().with(FILE, "old");
        let private = MemoryStorage::new().with(NAME, "new");
        let storage = Private::new(base, private);
        assert_eq!(read_file(&storage, FILE, 64).unwrap(), b"new");
        assert_eq!(storage.base.get(FILE).unwrap(), b"old");
    }

    #[test]
    fn they_go_with_their_folder() {
        let storage = Private::new(MemoryStorage::new(), MemoryStorage::new());
        storage.write(FILE, b"s").unwrap();
        storage.remove_dir_all(".paperpaw/plugins").unwrap();
        assert!(storage.exists(FILE));
        storage.remove_dir_all(".paperpaw").unwrap();
        assert!(!storage.exists(FILE));
        assert_eq!(
            storage.rename(FILE, "elsewhere.txt"),
            Err(StorageError::Unsupported)
        );
    }
}
