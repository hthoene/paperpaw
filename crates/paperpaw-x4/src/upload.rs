//! The debug remote's `put` (#106): a file written to the card under a
//! temporary name beside its target, and moved over the target only when it
//! is whole and on the card. A put that is cut off (a cable pulled, a
//! computer that stops talking, a reset) leaves the old file as it was; the
//! temporary file is taken away at the next put to the same name, or at
//! once where this side notices.

use alloc::string::String;
use core::fmt;

use paperpaw_platform::{Storage, StorageError};

/// What follows the target's name for the file in the making.
pub const SUFFIX: &str = ".pp-put";

/// A file being received.
pub struct Upload<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    temp: String,
    done: bool,
}

impl<'a> Upload<'a> {
    /// Starts the file for `path`: an empty temporary one. The target is
    /// not touched.
    pub fn begin(storage: &'a dyn Storage, path: &'a str) -> Result<Self, StorageError> {
        let temp = alloc::format!("{path}{SUFFIX}");
        storage.write(&temp, &[])?;
        Ok(Self {
            storage,
            path,
            temp,
            done: false,
        })
    }

    /// The next piece of the file.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        self.storage.append(&self.temp, bytes)
    }

    /// Puts the file in the place of the target, if it is `size` bytes and
    /// on the card. Otherwise the target stays as it is and the temporary
    /// file goes.
    pub fn finish(mut self, size: u64) -> Result<(), StorageError> {
        self.storage.sync()?;
        if self.storage.size(&self.temp)? != size {
            return Err(StorageError::InvalidData);
        }
        self.storage.rename(&self.temp, self.path)?;
        self.done = true;
        self.storage.sync()
    }
}

impl fmt::Debug for Upload<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Upload")
            .field("path", &self.path)
            .field("done", &self.done)
            .finish()
    }
}

impl Drop for Upload<'_> {
    /// What was not finished is not left behind.
    fn drop(&mut self) {
        if !self.done {
            let _ = self.storage.remove(&self.temp);
        }
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_sdk::MemoryStorage;

    use super::*;

    fn card() -> MemoryStorage {
        MemoryStorage::new().with("paperpaw.ini", b"[core]\nlanguage = de\n".to_vec())
    }

    fn read(storage: &MemoryStorage, path: &str) -> Option<alloc::vec::Vec<u8>> {
        let size = storage.size(path).ok()? as usize;
        let mut buf = alloc::vec![0u8; size];
        let n = storage.read_at(path, 0, &mut buf).ok()?;
        buf.truncate(n);
        Some(buf)
    }

    #[test]
    fn a_whole_file_replaces_the_old_one_at_the_end_only() {
        let storage = card();
        let mut put = Upload::begin(&storage, "paperpaw.ini").unwrap();
        put.push(b"[core]\n").unwrap();
        // Meanwhile the old file is whole.
        assert_eq!(
            read(&storage, "paperpaw.ini").unwrap(),
            b"[core]\nlanguage = de\n"
        );
        put.push(b"language = en\n").unwrap();
        put.finish(21).unwrap();
        assert_eq!(
            read(&storage, "paperpaw.ini").unwrap(),
            b"[core]\nlanguage = en\n"
        );
        assert!(read(&storage, "paperpaw.ini.pp-put").is_none());
    }

    #[test]
    fn a_put_that_is_cut_off_leaves_the_old_file_and_no_leftover() {
        let storage = card();
        {
            let mut put = Upload::begin(&storage, "paperpaw.ini").unwrap();
            put.push(b"[core]\n").unwrap();
            // The caller gives up here (silence, a bad line): dropped.
        }
        assert_eq!(
            read(&storage, "paperpaw.ini").unwrap(),
            b"[core]\nlanguage = de\n"
        );
        assert!(read(&storage, "paperpaw.ini.pp-put").is_none());
    }

    #[test]
    fn a_file_of_the_wrong_size_is_not_put_in_place() {
        let storage = card();
        let mut put = Upload::begin(&storage, "paperpaw.ini").unwrap();
        put.push(b"[core]\n").unwrap();
        assert_eq!(put.finish(21), Err(StorageError::InvalidData));
        assert_eq!(
            read(&storage, "paperpaw.ini").unwrap(),
            b"[core]\nlanguage = de\n"
        );
        assert!(read(&storage, "paperpaw.ini.pp-put").is_none());
    }

    #[test]
    fn a_new_file_and_an_empty_one_and_a_folder_that_is_not_there_yet() {
        let storage = MemoryStorage::new();
        let put = Upload::begin(&storage, "plugins/word/plugin.ini").unwrap();
        put.finish(0).unwrap();
        assert_eq!(read(&storage, "plugins/word/plugin.ini").unwrap(), b"");
        let mut put = Upload::begin(&storage, "a.txt").unwrap();
        put.push(b"abc").unwrap();
        put.finish(3).unwrap();
        assert_eq!(read(&storage, "a.txt").unwrap(), b"abc");
    }

    #[test]
    fn a_temporary_file_left_by_a_reset_is_started_over() {
        let storage = card().with("paperpaw.ini.pp-put", b"half of an old try".to_vec());
        let mut put = Upload::begin(&storage, "paperpaw.ini").unwrap();
        put.push(b"x = 1\n").unwrap();
        put.finish(6).unwrap();
        assert_eq!(read(&storage, "paperpaw.ini").unwrap(), b"x = 1\n");
    }
}
