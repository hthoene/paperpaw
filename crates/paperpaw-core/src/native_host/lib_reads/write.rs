//! The writes of `core.library` the catalog on the card backs: op 24
//! `provide` and the positions (ops 14). The contract's rules (flags, texts,
//! the position policy) are the service's; this maps what it has checked to
//! the card.
use super::{CatalogReads, position, provide};
use crate::native_host::lib_service::{BookRef, LibWrite, StoredPosition};
use crate::native_host::registrations as wire;

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
        position::write(self, book, written, from_sync)
    }

    fn provide(
        &self,
        book: BookRef<'_>,
        provided: &wire::ServiceLibProvided<'_>,
    ) -> Result<(), i32> {
        provide::provide(self, book, provided)
    }
}
