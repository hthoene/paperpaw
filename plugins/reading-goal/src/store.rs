//! `today.txt` in the plugin's data folder: what the statistics told last of
//! today (`<day> <seconds>`), read and written with no allocation at all.
//!
//! The names are the short ones of the native data folder (storage root 0).
//! The file is a few bytes, written in one call; the host's data storage
//! makes that call whole or not at all.

use paperpaw_sdk::native::Error;
use paperpaw_sdk::{Storage, StorageError};

use crate::model::{Kept, TODAY_FILE_MAX, parse_told, told_text};

/// The file, in the data folder.
pub const TODAY: &str = "today.txt";

/// What the file holds: the day and the seconds; nothing if it is missing,
/// too long (more than [`TODAY_FILE_MAX`] bytes) or not exactly that.
#[must_use]
pub fn read_told(storage: &dyn Storage) -> Option<(i32, u32)> {
    // One byte more than the most: a longer file is read as such.
    let mut text = [0u8; TODAY_FILE_MAX + 1];
    let n = storage.read_at(TODAY, 0, &mut text).ok()?;
    if n > TODAY_FILE_MAX {
        return None;
    }
    parse_told(text.get(..n)?)
}

fn storage_error(error: StorageError) -> Error {
    match error {
        StorageError::Full => Error::Full,
        _ => Error::Io,
    }
}

/// Writes what `kept` holds where the file does not hold it already; whether
/// it wrote.
///
/// # Errors
/// [`Error::Full`] for a full card, [`Error::Io`] for any other failure; the
/// file is as it was then.
pub fn flush(storage: &dyn Storage, kept: &Kept) -> Result<bool, Error> {
    let Some((day, seconds)) = kept.to_write(read_told(storage)) else {
        return Ok(false);
    };
    let mut out = [0u8; 32];
    let text = told_text(day, seconds, &mut out).ok_or(Error::NoSpace)?;
    storage
        .write(TODAY, text.as_bytes())
        .map_err(storage_error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn kept(day: i32, seconds: u32) -> Kept {
        let mut kept = Kept::new();
        let _ = kept.read_today(day, seconds);
        kept
    }

    #[test]
    fn what_was_told_is_written_once_and_read_back() {
        let storage = MemoryStorage::new();
        assert_eq!(read_told(&storage), None);
        assert_eq!(flush(&storage, &Kept::new()), Ok(false), "nothing told");
        assert_eq!(flush(&storage, &kept(20_000, 750)), Ok(true));
        assert_eq!(read_told(&storage), Some((20_000, 750)));
        assert_eq!(storage.get(TODAY).unwrap(), b"20000 750\n");
        assert_eq!(flush(&storage, &kept(20_000, 750)), Ok(false), "as it is");
        assert_eq!(flush(&storage, &kept(20_000, 760)), Ok(true));
        assert_eq!(read_told(&storage), Some((20_000, 760)));
        // A day before 1970 is written with its sign.
        assert_eq!(flush(&storage, &kept(-3, 5)), Ok(true));
        assert_eq!(read_told(&storage), Some((-3, 5)));
    }

    #[test]
    fn a_file_that_is_not_exactly_that_is_nothing() {
        let storage = MemoryStorage::new();
        for text in [
            &b"20000"[..],
            b"20000 7 8",
            b"x 7",
            b"20000 -7",
            b"\xff\xfe",
            b"",
        ] {
            storage.write(TODAY, text).unwrap();
            assert_eq!(read_told(&storage), None, "{text:?}");
        }
        // Longer than a file of ours can be.
        storage.write(TODAY, &[b'1'; TODAY_FILE_MAX + 1]).unwrap();
        assert_eq!(read_told(&storage), None);
        // A file that is wrong is written over, not kept.
        assert_eq!(flush(&storage, &kept(1, 2)), Ok(true));
        assert_eq!(read_told(&storage), Some((1, 2)));
    }

    #[test]
    fn a_failed_write_is_told_and_leaves_the_file() {
        struct Refuses(MemoryStorage, StorageError);
        impl Storage for Refuses {
            fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
                self.0.read_at(p, o, b)
            }
            fn size(&self, p: &str) -> Result<u64, StorageError> {
                self.0.size(p)
            }
            fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
                Err(self.1)
            }
            fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
                self.0.append(p, d)
            }
            fn list(
                &self,
                d: &str,
                v: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
            ) -> Result<(), StorageError> {
                self.0.list(d, v)
            }
            fn remove(&self, p: &str) -> Result<(), StorageError> {
                self.0.remove(p)
            }
        }
        let inner = MemoryStorage::new();
        inner.write(TODAY, b"1 2\n").unwrap();
        let full = Refuses(inner, StorageError::Full);
        assert_eq!(flush(&full, &kept(5, 6)), Err(Error::Full));
        assert_eq!(read_told(&full), Some((1, 2)));
        let broken = Refuses(MemoryStorage::new(), StorageError::Io);
        assert_eq!(flush(&broken, &kept(5, 6)), Err(Error::Io));
    }
}
