//! The store's data folder with folders in it: files through the storage slots
//! ([`DataStorage`]), folders through `core.files` on root 0, which the slots
//! have not (a download is staged in `stage/<plugin>/…`, as Beta 3's store
//! staged it). Without the folders every install stopped at its start
//! ("the card refused"), on every device.
use paperpaw_sdk::native::{
    Error, Host,
    files::{self, Files},
    storage::{DataStorage, Root},
};
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use paperpaw_ui_kit::mem::buffer;

/// [`DataStorage`] with folders.
#[derive(Clone, Copy)]
pub struct StoreStorage<'h, 'cb> {
    host: &'h Host<'cb>,
    data: DataStorage<'h, 'cb>,
}

impl core::fmt::Debug for StoreStorage<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoreStorage").finish_non_exhaustive()
    }
}

impl<'h, 'cb> StoreStorage<'h, 'cb> {
    /// The data folder through `host`.
    #[must_use]
    pub const fn new(host: &'h Host<'cb>) -> Self {
        Self {
            host,
            data: DataStorage::new(host),
        }
    }

    /// Runs `op` with the files service and its buffers (on the heap: too big
    /// for a callback's stack), opened for it and released after.
    fn with_files(
        &self,
        op: impl FnOnce(&Files, &mut [u8], &mut [u8]) -> Result<(), Error>,
    ) -> Result<(), StorageError> {
        let mut input = buffer(files::INPUT_BYTES).map_err(storage_error)?;
        let mut out = buffer(files::OUTPUT_BYTES).map_err(storage_error)?;
        let service = Files::open(self.host).map_err(storage_error)?;
        let done = op(&service, &mut input, &mut out);
        let released = service.release(self.host);
        done.and(released).map_err(storage_error)
    }
}

/// The host's refusal as a storage error, as [`DataStorage`] says it.
fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        Error::NoMemory => StorageError::OutOfMemory,
        _ => StorageError::Io,
    }
}

impl Storage for StoreStorage<'_, '_> {
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

    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.data.write_at(path, offset, data)
    }

    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.data.truncate(path, len)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.data.sync()
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.data.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.data.remove(path)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        self.data.modified(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.data.exists(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.data.rename(from, to)
    }

    /// The folder and its missing parents (`core.files` `make_dir`).
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.with_files(|service, input, out| {
            service.make_dir(self.host, Root::Data, path, input, out)
        })
    }

    /// The folder and all in it (`core.files` `remove_tree`).
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.with_files(|service, input, out| {
            service
                .remove_tree(self.host, Root::Data, path, input, out)
                .map(|_| ())
        })
    }
}
