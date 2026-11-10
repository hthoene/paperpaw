//! `core.library` op 24 `provide` over the catalog on the card
//! (`docs/abi/library.md` 5): what a source knows of a book the book's own
//! file does not name. The service has checked the record (the flags, blank
//! and control characters, the tags); this maps it to
//! `paperpaw_sdk::library::try_provide`, the one implementation: the values
//! are kept in the book's details where the file names nothing, the list of
//! tags replaces the book's, the book is read from its file anew, and an
//! error of the memory or the card is the answer, never a silent OK.
use super::{CatalogReads, fail};
use crate::native_host::lib_service::BookRef;
use crate::native_host::registrations as wire;
use paperpaw_sdk::library::{self, ProvidedText};

/// Op 24 `provide` over the catalog on the card.
pub(super) fn provide(
    reads: &CatalogReads,
    book: BookRef<'_>,
    provided: &wire::ServiceLibProvided<'_>,
) -> Result<(), i32> {
    // Bit n of `flags` names field n of the record, in its order.
    let named = |bit: u32, text| (provided.flags & (1 << bit) != 0).then_some(text);
    let text = ProvidedText {
        title: named(0, provided.title),
        author: named(1, provided.author),
        series: named(2, provided.series),
        series_index: named(3, provided.number),
        tags: named(4, provided.tags),
    };
    reads.with_path(book, |path| {
        library::try_provide(&*reads.storage, path, &text).map_err(fail)
    })
}

#[cfg(test)]
#[path = "provide_tests.rs"]
mod tests;
