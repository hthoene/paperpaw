//! Op 9 `can_delete` over the card (`docs/abi/library.md` 2.3): whether the
//! file at a root-1 path is a book that can be deleted, and what is kept for
//! it, so a view decides what to offer before it asks. It changes nothing.
//!
//! The book test is the one the catalog walk uses: a format the host's
//! format index knows by the file's extension ([`CatalogEnv::format_id`]), and
//! not in PaperPaw's own folder. A file without an extension is no book here
//! (no sniffing: `format_for` is the document host's, op 20).
use super::{CatalogReads, fail};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND, OK};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::{annotations, data, positions};

/// `lib_result.flags`: has annotations, has a position, has plugins' data.
const HAS_ANNOTATIONS: u32 = 1;
const HAS_POSITION: u32 = 1 << 1;
const HAS_BOOK_DATA: u32 = 1 << 2;

/// Whether `path` names a book by its format and place.
fn is_book(reads: &CatalogReads, path: &str) -> bool {
    let own = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(data::DIR));
    if own {
        return false;
    }
    let mut known = false;
    reads.env.format_id(path, &mut |_| known = true);
    known
}

/// The status and facts of op 9 for one path.
pub(super) fn can_delete(reads: &CatalogReads, path: &str) -> (i32, u32) {
    if !is_book(reads, path) {
        return (ERR_INVALID, 0);
    }
    match reads.storage.size(path) {
        Ok(_) => {}
        Err(StorageError::NotFound) => return (ERR_NOT_FOUND, 0),
        Err(StorageError::OutOfMemory) => return (ERR_NO_MEMORY, 0),
        Err(_) => return (ERR_IO, 0),
    }
    match facts(reads, path) {
        Ok(flags) => (OK, flags),
        Err(error) => (error, 0),
    }
}

fn facts(reads: &CatalogReads, path: &str) -> Result<u32, i32> {
    let storage: &dyn Storage = &*reads.storage;
    let mut flags = 0;
    // Any annotation row: the read stops at the first.
    let mut rows = 0_u32;
    annotations::try_rows(
        storage,
        path,
        &annotations::Windows::new(),
        &mut [],
        &mut |_, _| {
            rows += 1;
            core::ops::ControlFlow::Break(())
        },
    )
    .map_err(fail)?;
    if rows > 0 {
        flags |= HAS_ANNOTATIONS;
    }
    if positions::try_load(storage, path).map_err(fail)?.is_some() {
        flags |= HAS_POSITION;
    }
    if reads.env.book_data_kept(path)? {
        flags |= HAS_BOOK_DATA;
    }
    Ok(flags)
}

#[cfg(test)]
#[path = "delete_tests.rs"]
mod tests;
