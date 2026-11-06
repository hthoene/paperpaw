//! Path-based storage with caller-owned buffers. Root 0 is the plugin's data
//! folder. Root 1 reads the user's files only with signed `uses.storage` metadata;
//! the host enforces permissions. Root selection is passed through unchanged.
//! FAT paths ignore case. An append is durable only after [`sync`]; a failed
//! append may leave a partial tail, which the plugin's record format must handle.
use super::{Error, Host, binding::status};
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

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
