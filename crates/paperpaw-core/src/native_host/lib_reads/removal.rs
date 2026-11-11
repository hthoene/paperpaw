//! The catalog's side of a delete (`docs/abi/library.md` 2.3): what is kept
//! for a book that is gone from the card. The journalled job that removes the
//! files (Files' `LibraryMoves::begin_delete`) calls these in two phases after
//! the file is removed; every one is idempotent (a repeat after a cut finds it
//! done) and bounded (a few small files), and the card's or the memory's error
//! is the answer, for the caller to repeat the step:
//!
//! - [`details`]: the book's line in the catalog (the log of changes says it
//!   is removed), its details (title, tags and **fields**), its cover, its
//!   place among the recently opened and the favourites;
//! - [`marks`]: its position, its annotation file and the record of who
//!   imported it.
//!
//! Plugins' book data and the document caches are the job's own.
use super::{importer, kept_file, position};
use crate::native_host::lib_jobs::carry_status;
use paperpaw_platform::Storage;
use paperpaw_sdk::{library, library::catalog};

/// The folder of the annotation files (`paperpaw_sdk::annotations::file`).
const ANNOTATIONS: &str = ".paperpaw/annotations";

/// The catalog line, details, cover and lists of the book at `path`. A list
/// that cannot be read fails the step with `ERR_INVALID` (after the rest is
/// forgotten). Both parts always run, so that a damaged catalog does not keep
/// the details of the book on the card; the first error is the answer.
pub(crate) fn details(storage: &dyn Storage, path: &str) -> Result<(), i32> {
    let forgot = catalog::forget(storage, path).map_err(carry_status);
    // The SDK's one implementation, with its errors: the details (and the
    // fields), the cover (and the old names of both) and the two lists. A
    // card error is the answer, so that the step is repeated: details left
    // behind would be inherited by a later book at the same path.
    let removed = library::try_removed(storage, path).map_err(carry_status);
    forgot.and(removed)
}

/// The position, annotations and importer record of the book at `path`.
pub(crate) fn marks(storage: &dyn Storage, path: &str) -> Result<(), i32> {
    // A file that is not there is not an error (`Storage::remove`).
    storage
        .remove(&position::file(path)?)
        .map_err(carry_status)?;
    storage
        .remove(&kept_file(ANNOTATIONS, path)?)
        .map_err(carry_status)?;
    importer::remove(storage, path)
}

#[cfg(test)]
#[path = "removal_tests.rs"]
mod tests;
