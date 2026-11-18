//! The vocabulary's data layer on the host's data folder.
//!
//! The data layer names its files by their whole path under the card
//! (`.paperpaw/plugins/vocabulary/words.txt`, as interface 6 did); the native
//! storage's root 0 *is* that folder and takes names inside it. [`DataFolder`]
//! takes the folder off every path and passes the call on, so the layer runs
//! on the SDK's `native::storage::DataStorage` unchanged. It goes when
//! the layer's paths are the short ones (with the interface-6 callers).

use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// The folder root 0 stands for.
pub(crate) const FOLDER: &str = ".paperpaw/plugins/vocabulary/";

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

/// Where a file being written at `path` waits until it is whole.
pub(crate) fn part_of(path: &str) -> Result<alloc::string::String, StorageError> {
    let mut out = alloc::string::String::new();
    crate::lines::push(&mut out, path)?;
    crate::lines::push(&mut out, ".part")?;
    Ok(out)
}

/// Puts the file made at `part` at `path`, replacing what is there: what
/// was at `path` stays until the new file is whole. Where that fails, the
/// part is removed, unless the failed move took the old file away (FAT
/// removes it first): then the part is all there is.
pub(crate) fn put_in_place(
    storage: &dyn Storage,
    part: &str,
    path: &str,
) -> Result<(), StorageError> {
    let had_old = storage.exists(path);
    let moved = storage.rename(part, path);
    if moved.is_err() && (!had_old || storage.exists(path)) {
        let _ = storage.remove(part);
    }
    moved
}

/// Copies the file `from` to `to` through a part beside it, put in place when
/// whole: a copy cut short (a full card, a refused allocation) leaves no half
/// file that looks whole, and what was at `to` stays. The buffer is one
/// fallible allocation.
pub(crate) fn copy_file(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    let part = part_of(to)?;
    let mut chunk = alloc::vec::Vec::new();
    chunk
        .try_reserve_exact(COPY_CHUNK)
        .map_err(|_| StorageError::TooLarge)?;
    chunk.resize(COPY_CHUNK, 0u8);
    let copied = (|| {
        storage.write(&part, &[])?;
        let mut at = 0u64;
        loop {
            let n = storage.read_at(from, at, &mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            storage.append(&part, chunk.get(..n).ok_or(StorageError::Io)?)?;
            at += n as u64;
        }
    })();
    match copied {
        Ok(()) => put_in_place(storage, &part, to),
        Err(error) => {
            let _ = storage.remove(&part);
            Err(error)
        }
    }
}

/// What one read of a copy takes (the host takes 4 KiB a call).
const COPY_CHUNK: usize = 4096;

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
