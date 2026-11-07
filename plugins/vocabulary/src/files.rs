//! The vocabulary's data layer on the host's data folder.
//!
//! The data layer names its files by their whole path under the card
//! (`.paperpaw/plugins/vocabulary/words.txt`, as interface 6 did); the native
//! storage's root 0 *is* that folder and takes names inside it. [`DataFolder`]
//! takes the folder off every path and passes the call on, so the layer runs
//! on the SDK's `native::storage::DataStorage` unchanged. It goes when
//! the layer's paths are the short ones (with the interface-6 callers).

#![allow(
    dead_code,
    reason = "the native shell that builds DataFolder follows; until then only the tests do"
)]

use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// The folder root 0 stands for.
const FOLDER: &str = ".paperpaw/plugins/vocabulary/";

/// `inner`, seen from the plugin's data folder.
pub(crate) struct DataFolder<'a>(pub(crate) &'a dyn Storage);

/// The name inside the data folder; anything outside it is not ours.
fn inside(path: &str) -> Result<&str, StorageError> {
    path.strip_prefix(FOLDER).ok_or(StorageError::InvalidPath)
}

impl Storage for DataFolder<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(inside(path)?, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(inside(path)?)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(inside(path)?, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(inside(path)?, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The folder itself is the root: its listing is the inner root's.
        let inner = if dir.trim_end_matches('/') == FOLDER.trim_end_matches('/') {
            ""
        } else {
            inside(dir)?
        };
        self.0.list(inner, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(inside(path)?)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(inside(from)?, inside(to)?)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{self, Word};
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn the_data_layer_runs_on_a_folder_that_is_the_root() {
        let root = MemoryStorage::new();
        let folder = DataFolder(&root);
        let word = Word {
            word: "quire".into(),
            language: "en".into(),
            ..Word::default()
        };
        store::add(&folder, &word).unwrap();
        store::remove(&folder, &Word::default()).unwrap();
        // The names in the root are the short ones, and the rewrite's
        // temporary file is gone.
        assert!(root.get("words.txt").is_some());
        assert!(root.get("words.tmp").is_none());
        assert_eq!(store::count(&folder), Ok(1));
        assert_eq!(
            folder.size("other/words.txt"),
            Err(StorageError::InvalidPath)
        );
    }
}
