//! Op 8 `set_status` over the card (`docs/abi/library.md` 2.2): finished,
//! favourite and opened, both masks applied together. The service has checked
//! the masks (known bits, none in both); this maps them to
//! `paperpaw_sdk::library::try_set_status`, the one implementation (the same
//! files `set_finished` and `set_favourite` write for an interface 6 plugin),
//! so an error of the memory or the card is the answer, never a silent OK.
use super::{CatalogReads, fail};
use crate::native_host::lib_service::BookRef;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::library;

impl CatalogReads {
    /// Op 8 `set_status`, where `busy` says a book is in use (a library move
    /// carries its details and its places in the lists): `ERR_BUSY`, nothing
    /// changed, because the carry would put the old state over this one.
    pub(crate) fn set_status_unless(
        &self,
        book: BookRef<'_>,
        set: u32,
        clear: u32,
        busy: &dyn Fn(&str) -> bool,
    ) -> Result<(), i32> {
        self.with_path(book, |path| {
            if busy(path) {
                return Err(ERR_BUSY);
            }
            library::try_set_status(&*self.storage, path, set, clear).map_err(fail)
        })
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
