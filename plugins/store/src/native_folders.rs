//! The data folder with folders: [`DataStorage`] reads, writes, renames and
//! removes files in it but cannot make or remove a folder (`create_dir` and
//! `remove_dir_all` are `Unsupported` on it), and the download stages a plugin's
//! files in a folder of its own, so those two go through the host's `core.files`
//! (`make_dir` and `remove_tree` on root 0, `docs/abi/files.md` 2). Every other
//! call is the data storage's.
use paperpaw_sdk::native::{
    Error, Host,
    files::{Files, INPUT_BYTES, OUTPUT_BYTES},
    storage::{DataStorage, Root},
};
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use paperpaw_ui_kit::mem;

/// The plugin's data folder, as a [`Storage`] that makes and removes folders.
pub struct Folders<'a, 'h> {
    host: &'a Host<'h>,
    data: DataStorage<'a, 'h>,
}

impl core::fmt::Debug for Folders<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Folders").finish_non_exhaustive()
    }
}

impl<'a, 'h> Folders<'a, 'h> {
    /// The data folder of the plugin `host` runs.
    #[must_use]
    pub fn new(host: &'a Host<'h>) -> Self {
        Self {
            host,
            data: DataStorage::new(host),
        }
    }

    /// Runs `op` with the files service open; the handle is given back.
    fn files<R>(
        &self,
        op: impl FnOnce(&Files, &mut [u8], &mut [u8]) -> Result<R, Error>,
    ) -> Result<R, StorageError> {
        let mut input = mem::buffer(INPUT_BYTES).map_err(|_| StorageError::OutOfMemory)?;
        let mut out = mem::buffer(OUTPUT_BYTES).map_err(|_| StorageError::OutOfMemory)?;
        let opened = Files::open(self.host);
        let files = opened.map_err(error)?;
        let result = op(&files, &mut input, &mut out);
        files.release(self.host).ok();
        result.map_err(error)
    }
}

fn error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        Error::NoMemory => StorageError::OutOfMemory,
        _ => StorageError::Io,
    }
}

impl Storage for Folders<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.data.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.data.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.data.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.data.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.data.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.data.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.data.rename(from, to)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.data.sync()
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.files(|files, input, out| files.make_dir(self.host, Root::Data, path, input, out))
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.files(|files, input, out| files.remove_tree(self.host, Root::Data, path, input, out))
            .map(|_| ())
    }
}
