//! The production `core.library` host: the catalog reads over the card
//! ([`CatalogReads`]) and nothing else yet. The write half of the service is
//! not implemented in production (todo 307) but `provide` (the catalog
//! reads' own): every other write op answers `ERR_UNSUPPORTED`, the trait's
//! own default, so a plugin sees an honest "not here" rather than a
//! half-done change.
//!
//! The one required piece of the write trait is [`LibExcluded`]: a name
//! below the card's top is one of the host's folders (`plugins`, `.paperpaw`,
//! `paperpaw.ini`) by its spelling or, for an 8.3 alias, by identity, as
//! `core.files` decides it ([`excluded_on`]).
use super::lib_reads::{CatalogEnv, CatalogReads};
use super::lib_service::{BookRef, LibExcluded, LibQuery, LibWrite, One, Rows};
use super::registrations::{self as wire};
use super::storage_files::excluded_on;
use crate::abi7::context::RowsEnd;
use alloc::rc::Rc;
use paperpaw_platform::Storage;

/// `core.library` over the catalog on `storage`.
pub(crate) struct LibraryHost {
    reads: CatalogReads,
    storage: Rc<dyn Storage>,
}

impl LibraryHost {
    pub(crate) fn new(storage: Rc<dyn Storage>, env: Rc<dyn CatalogEnv>) -> Self {
        Self {
            reads: CatalogReads::new(storage.clone(), env),
            storage,
        }
    }
}

impl LibQuery for LibraryHost {
    fn state(&self, visit: One<'_>) -> Result<(), i32> {
        self.reads.state(visit)
    }
    fn book(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.reads.book(book, visit)
    }
    fn book_file_present(&self, path: &str) -> Result<bool, i32> {
        self.reads.book_file_present(path)
    }
    fn text(
        &self,
        book: BookRef<'_>,
        at: &wire::ServiceLibTextAt,
        visit: One<'_>,
    ) -> Result<(), i32> {
        self.reads.text(book, at, visit)
    }
    fn book_key(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.reads.book_key(book, visit)
    }
    fn position(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.reads.position(book, visit)
    }
    fn annotations(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.reads.annotations(book, cursor, generation, visit)
    }
    fn anchors(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.reads.anchors(book, cursor, generation, visit)
    }
}

/// The writes: `provide` is the catalog reads' own (it keeps what a source
/// provides for a book); every other op keeps its default (`ERR_UNSUPPORTED`)
/// until the production write half exists (todo 307).
impl LibWrite for LibraryHost {
    fn provide(
        &self,
        book: BookRef<'_>,
        provided: &wire::ServiceLibProvided<'_>,
    ) -> Result<(), i32> {
        self.reads.provide(book, provided)
    }
}

impl LibExcluded for LibraryHost {
    fn excluded(&self, first_segment: &str) -> bool {
        // Root 1 is the card's top: the parent is the empty path.
        excluded_on(&*self.storage, 1, "", first_segment)
    }
}

#[cfg(test)]
#[path = "lib_host_tests.rs"]
mod tests;
