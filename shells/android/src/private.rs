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

    fn list_bounded(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.base.list_bounded(dir, from, visit)
    }

    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.base.remove_listed(dir, place)
    }

    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        if root == FILE {
            // The secrets file itself is in the private folder, not in the
            // base: it is removed there, and done is told only then.
            let existed = self.private.exists(NAME);
            self.private.remove(NAME)?;
            return Ok(paperpaw_platform::tree::TreeStep {
                removed: u32::from(existed),
                done: true,
                next: 0,
            });
        }
        let mut step = self.base.remove_tree_step(root, keep_root, place)?;
        // The secrets go with the folder that holds them, as one removal of
        // the call's share: with the share used up it is left to the next
        // call (which finds the base's tree done at once), and `done` is told
        // only once the file is gone.
        let inside = root.is_empty()
            || FILE
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'));
        if step.done && inside && self.private.exists(NAME) {
            if step.removed >= paperpaw_platform::tree::MAX_REMOVALS {
                step.done = false;
                step.next = 1;
            } else {
                self.private.remove(NAME)?;
                step.removed += 1;
            }
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

    /// The step on the secrets file itself removes the file in the private
    /// folder (not in the base, where there is none); on a folder that holds
    /// it only once the whole folder is gone.
    #[test]
    fn a_tree_step_reaches_the_secrets_in_the_private_folder() {
        let storage = Private::new(MemoryStorage::new(), MemoryStorage::new());
        storage.write(FILE, b"s").unwrap();
        storage.write(".paperpaw/other.txt", b"o").unwrap();
        let step = storage.remove_tree_step(FILE, false, 0).unwrap();
        assert!(step.done && step.removed == 1);
        assert!(!storage.exists(FILE));
        assert!(storage.base.exists(".paperpaw/other.txt"));
        // Again: nothing there, still done.
        let step = storage.remove_tree_step(FILE, false, 0).unwrap();
        assert!(step.done && step.removed == 0);
    }

    /// A base whose tree steps are scripted: not done until the third call,
    /// each call removing `removed` entries.
    struct Scripted {
        calls: core::cell::Cell<u32>,
        removed: u32,
    }

    impl Scripted {
        fn new(removed: u32) -> Self {
            Self {
                calls: core::cell::Cell::new(0),
                removed,
            }
        }
    }

    impl Storage for Scripted {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            Ok(0)
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Err(StorageError::NotFound)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove_tree_step(
            &self,
            _: &str,
            _: bool,
            _: u64,
        ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
            let calls = self.calls.get() + 1;
            self.calls.set(calls);
            Ok(paperpaw_platform::tree::TreeStep {
                // Once done, a tree has nothing left to remove.
                removed: if calls > 3 { 0 } else { self.removed },
                done: calls >= 3,
                next: u64::from(calls),
            })
        }
    }

    /// A private folder whose file cannot be removed.
    struct Stuck;

    impl Storage for Stuck {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            Ok(0)
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Ok(1)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
    }

    /// The secrets go with the folder that holds them, in the step that
    /// finishes the tree, not before; a folder that does not hold them never
    /// reaches them.
    #[test]
    fn a_tree_step_takes_the_secrets_with_the_last_piece_of_their_folder() {
        for (root, holds) in [
            (".paperpaw", true),
            ("", true),
            (".paperpaw/plugins", false),
            ("Books", false),
        ] {
            let storage = Private::new(Scripted::new(1), MemoryStorage::new());
            storage.private.write(NAME, b"s").unwrap();
            for piece in 1..=3 {
                let step = storage.remove_tree_step(root, false, 0).unwrap();
                assert_eq!(step.done, piece == 3);
                let kept = !(holds && step.done);
                assert_eq!(storage.exists(FILE), kept, "{root:?} piece {piece}");
            }
        }
    }

    /// The removal of the secrets is one removal of the call's share: with the
    /// share used up by the base it is left to the next call, counted when it
    /// is made, and `done` is told only once the file is gone.
    #[test]
    fn the_secrets_are_one_removal_of_the_share() {
        use paperpaw_platform::tree::MAX_REMOVALS;
        // The share is used up, the file is there: not done after the base.
        let storage = Private::new(Scripted::new(MAX_REMOVALS), MemoryStorage::new());
        storage.private.write(NAME, b"s").unwrap();
        let mut counted = 0;
        let mut pieces = 0;
        loop {
            let step = storage.remove_tree_step(".paperpaw", false, 0).unwrap();
            assert!(step.removed <= MAX_REMOVALS, "{}", step.removed);
            counted += step.removed;
            pieces += 1;
            if step.done {
                break;
            }
            assert!(pieces < 10);
        }
        assert!(!storage.exists(FILE));
        // Three pieces of the base (64 each), and the file as one removal more.
        assert_eq!(counted, 3 * MAX_REMOVALS + 1);
        // Below the share it goes along, counted.
        let storage = Private::new(Scripted::new(MAX_REMOVALS - 1), MemoryStorage::new());
        storage.private.write(NAME, b"s").unwrap();
        let mut last = None;
        for _ in 0..3 {
            last = Some(storage.remove_tree_step(".paperpaw", false, 0).unwrap());
        }
        let last = last.unwrap();
        assert!(last.done && last.removed == MAX_REMOVALS);
        assert!(!storage.exists(FILE));
        // No file: the base's count as it is, done at once.
        let storage = Private::new(Scripted::new(MAX_REMOVALS), MemoryStorage::new());
        for _ in 0..2 {
            assert!(
                !storage
                    .remove_tree_step(".paperpaw", false, 0)
                    .unwrap()
                    .done
            );
        }
        let last = storage.remove_tree_step(".paperpaw", false, 0).unwrap();
        assert!(last.done && last.removed == MAX_REMOVALS);
    }

    /// A file that cannot be removed is an error, never `done`.
    #[test]
    fn a_secret_that_stays_is_never_reported_done() {
        for removed in [3, paperpaw_platform::tree::MAX_REMOVALS] {
            let storage = Private::new(Scripted::new(removed), Stuck);
            let mut outcomes = Vec::new();
            for _ in 0..4 {
                outcomes.push(
                    storage
                        .remove_tree_step(".paperpaw", false, 0)
                        .map(|s| s.done),
                );
            }
            assert!(
                outcomes.iter().all(|o| *o != Ok(true)),
                "{removed}: {outcomes:?}"
            );
            assert_eq!(outcomes.last(), Some(&Err(StorageError::Io)), "{removed}");
        }
    }
}
