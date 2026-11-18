//! The installed digests needed for one startup, read together.

use alloc::vec::Vec;
use paperpaw_platform::{Storage, StorageError};

use super::{INSTALLED, entry_in};
use crate::Id;

/// A snapshot of installed code digests for the plugins a kernel will load.
/// Read after pending installations have settled. It is discarded after
/// startup, so a later installation is read afresh on the next start.
/// Only requested IDs and digests are kept; the source file is released
/// before the caller loads any code.
#[derive(Debug, Default)]
pub struct InstalledCodes {
    entries: Vec<(Id, [u8; 32])>,
}

impl InstalledCodes {
    /// Reads the same bounded file as [`super::installed_code`], once for
    /// all `plugins`. Missing or malformed entries grant no permission.
    /// An unreadable, oversized or non-UTF-8 file returns an error.
    pub fn load(
        storage: &dyn Storage,
        plugins: impl IntoIterator<Item = Id>,
    ) -> Result<Self, StorageError> {
        let mut plugins = plugins.into_iter().peekable();
        if plugins.peek().is_none() {
            return Ok(Self::default());
        }
        let bytes = crate::read_file(storage, INSTALLED, 64 * 1024)?;
        let text = core::str::from_utf8(&bytes).map_err(|_| StorageError::InvalidData)?;
        let mut entries = Vec::new();
        for plugin in plugins {
            if let Some(digest) = entry_in(text, plugin.as_str()) {
                entries
                    .try_reserve(1)
                    .map_err(|_| StorageError::OutOfMemory)?;
                entries.push((plugin, digest));
            }
        }
        Ok(Self { entries })
    }

    /// The digest recorded for a requested plugin at the time of loading.
    pub fn get(&self, plugin: impl AsRef<str>) -> Option<[u8; 32]> {
        self.entries
            .iter()
            .find(|(id, _)| id.as_str() == plugin.as_ref())
            .map(|(_, digest)| *digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::native::{installed_code, remember_code};

    const CLOCK: Id = Id::new_static("clock");
    const WEATHER: Id = Id::new_static("weather");

    #[test]
    fn only_requested_digests_survive_the_snapshot() {
        let storage = MemoryStorage::new();
        assert!(remember_code(&storage, "clock", &[1; 32]));
        assert!(remember_code(&storage, "weather", &[2; 32]));
        // An unrelated large part of the file must not stay in the snapshot.
        storage
            .append(INSTALLED, "#".repeat(60 * 1024).as_bytes())
            .unwrap();
        let codes = InstalledCodes::load(&storage, [CLOCK]).unwrap();
        assert_eq!(codes.get("clock"), Some([1; 32]));
        assert_eq!(codes.get("weather"), None);
        assert_eq!(codes.entries.len(), 1);
        assert!(codes.entries.capacity() * core::mem::size_of::<(Id, [u8; 32])>() < 512);
    }

    #[test]
    fn snapshot_and_single_lookup_use_the_same_first_entry_rules() {
        let hex = paperpaw_digest::hex::encode(&[3; 32]);
        let source =
            alloc::format!("clock-extra\t{hex}\nclock\tbroken\nclock\t{hex}\nweather\t  {hex}  \n");
        let storage = MemoryStorage::new().with(INSTALLED, source);
        let codes = InstalledCodes::load(&storage, [CLOCK, WEATHER]).unwrap();
        for id in ["clock", "weather", "absent"] {
            assert_eq!(codes.get(id), installed_code(&storage, id));
        }
        assert_eq!(codes.get("clock"), None);
        assert_eq!(codes.get("weather"), Some([3; 32]));
    }

    #[test]
    fn each_start_reads_changed_permissions_afresh() {
        let storage = MemoryStorage::new();
        assert!(remember_code(&storage, "clock", &[1; 32]));
        let first = InstalledCodes::load(&storage, [CLOCK]).unwrap();
        assert!(remember_code(&storage, "clock", &[2; 32]));
        let next = InstalledCodes::load(&storage, [CLOCK]).unwrap();
        assert_eq!(first.get("clock"), Some([1; 32]));
        assert_eq!(next.get("clock"), Some([2; 32]));
    }

    #[test]
    fn invalid_or_unreadable_lists_grant_no_permissions() {
        let missing = MemoryStorage::new();
        assert!(matches!(
            InstalledCodes::load(&missing, [CLOCK]),
            Err(StorageError::NotFound)
        ));
        let invalid = MemoryStorage::new().with(INSTALLED, [0xff]);
        assert!(matches!(
            InstalledCodes::load(&invalid, [CLOCK]),
            Err(StorageError::InvalidData)
        ));
        let large = MemoryStorage::new().with(INSTALLED, "x".repeat(64 * 1024 + 1));
        assert!(matches!(
            InstalledCodes::load(&large, [CLOCK]),
            Err(StorageError::TooLarge)
        ));
        let failing = crate::memory_storage::Unreadable(invalid);
        assert!(matches!(
            InstalledCodes::load(&failing, [CLOCK]),
            Err(StorageError::Io)
        ));
        assert!(
            InstalledCodes::load(&failing, [])
                .unwrap()
                .entries
                .is_empty()
        );
    }
}
