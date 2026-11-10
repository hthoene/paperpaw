//! Op 23 `set_field` over the catalog on the card (`docs/abi/library.md` 4):
//! a field `<plugin>.<name>` the plugin declared, kept with the book's data.
//! The service has checked the record (own field, the 64 bytes of an
//! in-catalog value); this maps it to `paperpaw_sdk::library::try_set_field`,
//! the one implementation, so an interface 6 reader sees the same details.
//! An error of the memory or the card is the answer, never a silent OK.
use super::{CatalogReads, fail};
use crate::native_host::lib_service::{BookRef, FieldKind};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND};
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

impl CatalogReads {
    /// Op 23 `set_field` (an empty `value` removes the field), where `busy`
    /// says a book is in use (a library move carries it): `ERR_BUSY`, nothing
    /// kept, because the carry would put the old details over the field.
    pub(crate) fn set_field_unless(
        &self,
        book: BookRef<'_>,
        field: &str,
        value: &str,
        busy: &dyn Fn(&str) -> bool,
    ) -> Result<(), i32> {
        self.with_path(book, |path| {
            if busy(path) {
                return Err(ERR_BUSY);
            }
            library::try_set_field(&*self.storage, path, field, value).map_err(fail)
        })
    }
}

#[cfg(test)]
#[path = "field_tests.rs"]
mod tests;
