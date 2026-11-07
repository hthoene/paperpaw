//! Path-based storage with caller-owned buffers. Root 0 is the plugin's data
//! folder. Root 1 reads the user's files only with signed `uses.storage` metadata;
//! the host enforces permissions. Root selection is passed through unchanged.
//! FAT paths ignore case. An append is durable only after [`sync`]; a failed
//! append may leave a partial tail, which the plugin's record format must handle.
use super::{Error, Host, binding::status};
use crate::{DirEntry, Storage, StorageError};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// The storage namespace selected by the plugin. Permissions are enforced by
/// the host; Files needs a signed `uses.storage` declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Root {
    /// This plugin's data folder (wire root 0).
    Data,
    /// User files on the card (wire root 1).
    Files,
}
impl Root {
    const fn wire(self) -> u32 {
        match self {
            Self::Data => 0,
            Self::Files => 1,
        }
    }
}

/// Maximum path length, in UTF-8 bytes.
pub const MAX_PATH: usize = 512;
/// Maximum path segment length, in UTF-8 bytes.
pub const MAX_SEGMENT: usize = 255;
/// Maximum bytes per read, write or append call.
pub const MAX_DATA: usize = 4096;

fn path(text: &str) -> Result<PpBytes, Error> {
    if text.len() > MAX_PATH || text.split('/').any(|s| s.len() > MAX_SEGMENT) {
        return Err(Error::NoSpace);
    }
    if !paperpaw_platform::storage::is_valid_path(text) {
        return Err(Error::Invalid);
    }
    Ok(bytes(text.as_bytes()))
}
fn bytes(input: &[u8]) -> PpBytes {
    PpBytes {
        ptr: input.as_ptr(),
        len: input.len(),
    }
}
fn data(input: &[u8]) -> Result<PpBytes, Error> {
    if input.len() > MAX_DATA {
        return Err(Error::NoSpace);
    }
    Ok(bytes(input))
}

/// Read a file's size without allocating.
///
/// # Errors
/// Returns path validation errors or the host refusal, including missing files.
pub fn size(host: &Host<'_>, root: Root, name: impl AsRef<str>) -> Result<u64, Error> {
    let path = path(name.as_ref())?;
    let mut out = 0;
    // SAFETY: callback binding and input are live; output is local and disjoint.
    status(unsafe { (host.table.storage_size)(host.ctx, root.wire(), path, &mut out) })?;
    Ok(out)
}

/// Copy at most `out.len()` bytes at `offset`, returning the count read.
/// Short reads, including zero at the end, succeed.
///
/// # Errors
/// Returns validation errors or a host refusal. Over-cap buffers are refused
/// before calling the host; a host count beyond the supplied buffer is invalid.
pub fn read_at(
    host: &Host<'_>,
    root: Root,
    name: impl AsRef<str>,
    offset: u64,
    out: &mut [u8],
) -> Result<usize, Error> {
    let path = path(name.as_ref())?;
    if out.len() > MAX_DATA {
        return Err(Error::NoSpace);
    }
    // SAFETY: the binding and path live; output is exclusively borrowed.
    let n = unsafe {
        (host.table.storage_read_at)(
            host.ctx,
            root.wire(),
            path,
            offset,
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    let n = usize::try_from(n).map_err(|_| Error::from_status(n))?;
    if n > out.len() {
        return Err(Error::Invalid);
    }
    Ok(n)
}

/// Atomically replace a file with at most 4096 bytes. Empty data creates an
/// empty file. Parent folders under the plugin root are created by the host.
///
/// # Errors
/// Returns validation or host errors. A refused call leaves the old file intact.
pub fn write(
    host: &Host<'_>,
    root: Root,
    name: impl AsRef<str>,
    input: &[u8],
) -> Result<(), Error> {
    let path = path(name.as_ref())?;
    let data = data(input)?;
    // SAFETY: all inputs and the callback binding remain live for this call.
    status(unsafe { (host.table.storage_write)(host.ctx, root.wire(), path, data) })
}

/// Append at most 4096 bytes; empty data creates the file. Call [`sync`] for
/// durability. Chunk text logs on record boundaries, and detect torn tails.
///
/// # Errors
/// Returns validation or host errors. After Full or Io, a partial tail may
/// remain: end that fragment before appending the next record.
pub fn append(
    host: &Host<'_>,
    root: Root,
    name: impl AsRef<str>,
    input: &[u8],
) -> Result<(), Error> {
    let path = path(name.as_ref())?;
    let data = data(input)?;
    // SAFETY: all inputs and the callback binding remain live for this call.
    status(unsafe { (host.table.storage_append)(host.ctx, root.wire(), path, data) })
}

/// Rename within one root, replacing the destination file if present.
///
/// # Errors
/// Returns validation or host errors; both paths are checked before the call.
pub fn rename(
    host: &Host<'_>,
    root: Root,
    from: impl AsRef<str>,
    to: impl AsRef<str>,
) -> Result<(), Error> {
    let from = path(from.as_ref())?;
    let to = path(to.as_ref())?;
    // SAFETY: both paths and the callback binding remain live for this call.
    status(unsafe { (host.table.storage_rename)(host.ctx, root.wire(), from, to) })
}

/// Remove a file. Removing an absent file succeeds.
///
/// # Errors
/// Returns validation or host errors.
pub fn remove(host: &Host<'_>, root: Root, name: impl AsRef<str>) -> Result<(), Error> {
    let path = path(name.as_ref())?;
    // SAFETY: the path and callback binding remain live for this call.
    status(unsafe { (host.table.storage_remove)(host.ctx, root.wire(), path) })
}

/// Flush pending writes on the device. The host meters each attempt per plugin.
///
/// # Errors
/// Returns the host error; callers must not claim durability after failure.
pub fn sync(host: &Host<'_>) -> Result<(), Error> {
    // SAFETY: the callback binding retains the table and context.
    status(unsafe { (host.table.storage_sync)(host.ctx) })
}

/// The plugin's data folder (root 0) as a [`Storage`](crate::Storage), for the
/// code that takes one (a database over a file, a record store). Bigger reads,
/// writes and appends are split into calls of at most [`MAX_DATA`] bytes: a
/// write of more replaces the file with its first piece and appends the rest,
/// so a caller that needs all or nothing writes beside the place and renames.
/// Removing a missing file is not an error. Folders are `core.files`:
/// [`list`](crate::Storage::list) says `Unsupported`. Nothing allocates.
#[derive(Clone, Copy)]
pub struct DataStorage<'h, 'cb> {
    host: &'h Host<'cb>,
}

impl core::fmt::Debug for DataStorage<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataStorage").finish_non_exhaustive()
    }
}

impl<'h, 'cb> DataStorage<'h, 'cb> {
    /// The data folder through `host`.
    #[must_use]
    pub const fn new(host: &'h Host<'cb>) -> Self {
        Self { host }
    }

    fn append_pieces(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        for piece in data.chunks(MAX_DATA) {
            append(self.host, Root::Data, path, piece).map_err(storage_error)?;
        }
        Ok(())
    }
}

/// The host's refusal as a storage error; a refused allocation is
/// `Other(1)`.
fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        Error::NoMemory => StorageError::Other(1),
        _ => StorageError::Io,
    }
}

impl Storage for DataStorage<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let mut done = 0;
        while done < buf.len() {
            let end = buf.len().min(done + MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n = read_at(self.host, Root::Data, path, at, part).map_err(storage_error)?;
            done += n;
            // A short read is the end (or all the host has for now).
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        size(self.host, Root::Data, path).map_err(storage_error)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (first, rest) = data.split_at(data.len().min(MAX_DATA));
        write(self.host, Root::Data, path, first).map_err(storage_error)?;
        self.append_pieces(path, rest)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if data.is_empty() {
            // Creates the file where there is none.
            return append(self.host, Root::Data, path, data).map_err(storage_error);
        }
        self.append_pieces(path, data)
    }

    fn list(&self, _dir: &str, _visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        match remove(self.host, Root::Data, path) {
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        rename(self.host, Root::Data, from, to).map_err(storage_error)
    }

    fn sync(&self) -> Result<(), StorageError> {
        sync(self.host).map_err(storage_error)
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "data_storage_tests.rs"]
mod data_storage_tests;
