//! The plugins' own storages over the host's storage calls (the formats' book
//! and cache, the dictionary's roots, the card adapters): one mapping of the
//! host's refusals and one read in 4 KiB calls, instead of a copy in each
//! (#492). The mapping is the SDK's `DataStorage`'s; change both together.
use paperpaw_sdk::StorageError;
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, MAX_DATA, Root},
};

/// The host's refusal as a storage error. A refused allocation is
/// `OutOfMemory` (never `Other(1)`, the card taken out); `Denied` is a change
/// in a read-only root, `Unsupported`; `NoSpace` is a path over its caps (data
/// goes in pieces that fit, and a full card is `Full`), `InvalidPath`. Where a
/// root gives `NoSpace` another meaning, see [`error_in`]; `core.files` tells
/// a folder too deep with it too.
#[must_use]
pub fn error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full => StorageError::Full,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid | Error::NoSpace | Error::Utf8 => StorageError::InvalidPath,
        Error::NoMemory => StorageError::OutOfMemory,
        _ => StorageError::Io,
    }
}

/// The host's refusal of a call on `root`: as [`error`], except that the
/// document cache's `NoSpace` is its quota (`docs/abi/document.md`), `Full`.
#[must_use]
pub fn error_in(root: Root, refusal: Error) -> StorageError {
    match (root, refusal) {
        (Root::DocumentCache, Error::NoSpace) => StorageError::Full,
        (_, refusal) => error(refusal),
    }
}

/// `storage::read_at` into a buffer of any size, in calls of at most
/// [`MAX_DATA`] bytes; the count read. A short read is the end (or all the
/// host has for now): nothing more is asked.
///
/// # Errors
/// The first refusal, as `storage::read_at` gives it.
pub fn read_full(
    host: &Host<'_>,
    root: Root,
    name: &str,
    offset: u64,
    buf: &mut [u8],
) -> Result<usize, Error> {
    let mut done = 0;
    while done < buf.len() {
        let end = buf.len().min(done + MAX_DATA);
        let Some(part) = buf.get_mut(done..end) else {
            break;
        };
        let at = offset.saturating_add(done as u64);
        let n = storage::read_at(host, root, name, at, part)?;
        done += n;
        if n < part.len() {
            break;
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_host_refusal_is_one_storage_error() {
        for (refusal, expected) in [
            (Error::NotFound, StorageError::NotFound),
            (Error::Full, StorageError::Full),
            (Error::Unsupported, StorageError::Unsupported),
            (Error::Denied, StorageError::Unsupported),
            (Error::Invalid, StorageError::InvalidPath),
            (Error::NoSpace, StorageError::InvalidPath),
            (Error::Utf8, StorageError::InvalidPath),
            (Error::NoMemory, StorageError::OutOfMemory),
            (Error::Io, StorageError::Io),
            (Error::Busy, StorageError::Io),
            (Error::Other(-99), StorageError::Io),
        ] {
            assert_eq!(error(refusal), expected, "{refusal:?}");
            assert_eq!(error_in(Root::Book, refusal), expected, "{refusal:?}");
        }
    }

    #[test]
    fn the_document_cache_over_its_quota_is_full_not_a_bad_path() {
        assert_eq!(
            error_in(Root::DocumentCache, Error::NoSpace),
            StorageError::Full
        );
        assert_eq!(
            error_in(Root::DocumentCache, Error::NoMemory),
            StorageError::OutOfMemory
        );
    }
}
