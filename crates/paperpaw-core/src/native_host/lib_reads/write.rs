//! The writes of `core.library` the catalog on the card backs: op 24
//! `provide`, the positions (op 14) and `set_field` (op 23). The contract's rules (flags, texts,
//! the position policy) are the service's; this maps what it has checked to
//! the card.
use super::{CatalogReads, delete, field, position};
use crate::native_host::lib_service::{BookRef, FieldKind, LibWrite, StoredPosition};
use crate::native_host::registrations as wire;

impl CatalogReads {
    /// Op 14 `write_position`, where `busy` says a book is in use (a library
    /// move carries it): `ERR_BUSY`, nothing written.
    pub(crate) fn write_position_unless(
        &self,
        book: BookRef<'_>,
        written: &wire::ServiceLibPosition<'_>,
        from_sync: bool,
        busy: &dyn Fn(&str) -> bool,
    ) -> Result<(), i32> {
        position::write(self, book, written, from_sync, busy)
    }
}

impl LibWrite for CatalogReads {
    fn stored_position(&self, book: BookRef<'_>) -> Result<Option<StoredPosition>, i32> {
        position::stored(self, book)
    }

    fn write_position(
        &self,
        book: BookRef<'_>,
        written: &wire::ServiceLibPosition<'_>,
        from_sync: bool,
    ) -> Result<(), i32> {
        position::write(self, book, written, from_sync, &|_| false)
    }

    fn provide(
        &self,
        book: BookRef<'_>,
        provided: &wire::ServiceLibProvided<'_>,
    ) -> Result<(), i32> {
        self.provide_unless(book, provided, &|_| false)
    }

    fn field_kind(&self, name: &str) -> Result<FieldKind, i32> {
        field::kind(self, name)
    }

    fn set_field(&self, book: BookRef<'_>, name: &str, value: &str) -> Result<(), i32> {
        self.set_field_unless(book, name, value, &|_| false)
    }

    fn set_status(&self, book: BookRef<'_>, set: u32, clear: u32) -> Result<(), i32> {
        self.set_status_unless(book, set, clear, &|_| false)
    }

    fn can_delete(&self, path: &str) -> (i32, u32) {
        delete::can_delete(self, path)
    }
}
