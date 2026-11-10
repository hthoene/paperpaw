//! Op 23 `set_field` over the catalog on the card (`docs/abi/library.md` 4):
//! a field `<plugin>.<name>` the plugin declared, kept with the book's data.
//! The service has checked the record (own field, the 64 bytes of an
//! in-catalog value); this maps it to `paperpaw_sdk::library::try_set_field`,
//! the one implementation, so an interface 6 reader sees the same details.
//! An error of the memory or the card is the answer, never a silent OK.
use super::{CatalogReads, fail};
use crate::native_host::lib_service::{BookRef, FieldKind};
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::library;

/// Op 23's declared-field lookup: `ERR_NOT_FOUND` for a field no plugin
/// declared.
pub(super) fn kind(reads: &CatalogReads, field: &str) -> Result<FieldKind, i32> {
    reads
        .env
        .book_field(field)
        .map(|in_catalog| FieldKind { in_catalog })
        .ok_or(ERR_NOT_FOUND)
}

/// Op 23 `set_field`: an empty `value` removes the field.
pub(super) fn set(
    reads: &CatalogReads,
    book: BookRef<'_>,
    field: &str,
    value: &str,
) -> Result<(), i32> {
    reads.with_path(book, |path| {
        library::try_set_field(&*reads.storage, path, field, value).map_err(fail)
    })
}

#[cfg(test)]
#[path = "field_tests.rs"]
mod tests;
