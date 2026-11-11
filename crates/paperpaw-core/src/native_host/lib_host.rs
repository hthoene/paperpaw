//! The production `core.library` host: the catalog reads over the card
//! ([`CatalogReads`]) and the writes the catalog on the card backs: `provide`
//! (op 24), the positions (op 14), `set_field` (op 23) and the delete
//! preflight `can_delete` (op 9) and `set_status` (op 8). Every other write
//! op answers `ERR_UNSUPPORTED`, the trait's own default, so a plugin sees an
//! honest "not here" rather than a half-done change (todo 307).
//!
//! The one required piece of the write trait is [`LibExcluded`]: a name
//! below the card's top is one of the host's folders (`plugins`, `.paperpaw`,
//! `paperpaw.ini`) by its spelling or, for an 8.3 alias (a name with a `~`
//! where one of those names also exists), as `core.files` decides it
//! ([`excluded_on`]; it over-refuses where it cannot tell).
use super::files_service::Access;
use super::lib_changed::{self, Changes};
use super::lib_edits::LibEdits;
use super::lib_moves_service::LibraryMoves;
use super::lib_reads::{CatalogEnv, CatalogReads};
use super::lib_service::{
    BookId, BookRef, Caller, CommitRequest, Committed, FieldKind, LibExcluded, LibQuery, LibWrite,
    One, Rows, StageItem, StoredPosition, Transactions,
};
use super::registrations::{self as wire};
use super::storage_files::excluded_on;
use crate::abi7::context::RowsEnd;
use alloc::rc::Rc;
use paperpaw_platform::Storage;

/// `core.library` over the catalog on `storage`.
pub(crate) struct LibraryHost {
    reads: CatalogReads,
    storage: Rc<dyn Storage>,
    /// The library's move job, where `core.files` is wired too: a book it
    /// carries is in use.
    moves: Option<Rc<LibraryMoves>>,
    /// The staged annotation transactions (ops 17 to 21 and 26).
    edits: LibEdits,
    /// What changed, for `event.library_changed` (the kernel's pump takes it).
    changes: Changes,
}

impl LibraryHost {
    pub(crate) fn new(
        storage: Rc<dyn Storage>,
        env: Rc<dyn CatalogEnv>,
        moves: Option<Rc<LibraryMoves>>,
    ) -> Self {
        Self {
            reads: CatalogReads::new(storage.clone(), env.clone()),
            edits: LibEdits::new(storage.clone(), env),
            storage,
            moves,
            changes: Changes::default(),
        }
    }

    /// Notes that `what` (the bits of [`super::lib_changed`]) changed for the
    /// book at `path` (empty: no one book) so that the plugins that subscribed
    /// to `event.library_changed` are told, the changer too. Called after the
    /// change is on the card; never fails, never calls a plugin.
    pub(crate) fn raise_changed(&self, what: u32, path: &str) {
        self.changes.raise(what, path);
    }
}

impl LibraryHost {
    /// Reads the device's memory class and clock from what the other wirings
    /// said ([`EditPolicy`]), whenever they came.
    #[must_use]
    pub(crate) fn with_policy(mut self, policy: Rc<super::lib_edits::EditPolicy>) -> Self {
        self.edits = self.edits.with_policy(policy);
        self
    }

    /// Whether the library's move job holds the book at `path`.
    fn in_use(&self, path: &str) -> bool {
        self.moves
            .as_ref()
            .is_some_and(|moves| moves.moves().busy(&**moves.card(), path, Access::Rework))
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

/// The writes the catalog on the card backs: `provide` (op 24), the positions
/// (ops 13/14's write half) and `set_field` (op 23). Each raises its bit of
/// `event.library_changed` once it is on the card, with no one book named (the
/// plugin re-reads what it shows). Every other op keeps its
/// default (`ERR_UNSUPPORTED`) until the rest of the write half exists
/// (todo 307).
impl LibWrite for LibraryHost {
    fn provide(
        &self,
        book: BookRef<'_>,
        provided: &wire::ServiceLibProvided<'_>,
    ) -> Result<(), i32> {
        // A book the move job carries is `ERR_BUSY`: the carry would put the
        // old details over what a source provided, although it answered OK.
        self.reads
            .provide_unless(book, provided, &|path| self.in_use(path))?;
        self.raise_changed(lib_changed::FIELDS, "");
        Ok(())
    }

    fn stored_position(&self, book: BookRef<'_>) -> Result<Option<StoredPosition>, i32> {
        self.reads.stored_position(book)
    }

    /// A local or confirmed position is the book's own: the carry never
    /// replaces it, so a write during a move survives. An unconfirmed sync
    /// write is `ERR_BUSY` while the move holds the book: before the carry
    /// `stored_position` finds nothing at the new key, so it could land there
    /// over the user's local position (`library.md` 2.2: a local write wins).
    fn write_position(
        &self,
        book: BookRef<'_>,
        position: &wire::ServiceLibPosition<'_>,
        from_sync: bool,
    ) -> Result<(), i32> {
        self.reads
            .write_position_unless(book, position, from_sync, &|path| {
                from_sync && self.in_use(path)
            })?;
        self.raise_changed(lib_changed::POSITIONS, "");
        Ok(())
    }

    /// Status is part of the details and the lists a move carries: a book the
    /// job holds is `ERR_BUSY` (as for `set_field`).
    fn set_status(&self, book: BookRef<'_>, set: u32, clear: u32) -> Result<(), i32> {
        self.reads
            .set_status_unless(book, set, clear, &|path| self.in_use(path))?;
        self.raise_changed(lib_changed::STATUS, "");
        Ok(())
    }

    /// Op 9's preflight: the card's, read-only (a book, gone, what is kept).
    fn can_delete(&self, path: &str) -> (i32, u32) {
        self.reads.can_delete(path)
    }

    fn field_kind(&self, field: &str) -> Result<FieldKind, i32> {
        self.reads.field_kind(field)
    }

    /// The book's key is its identity for a transaction. A book the move job
    /// holds is `ERR_BUSY`: the carry would put the old annotations over the
    /// new ones. (A move cannot start while a transaction is open on the
    /// book: the service refuses it, so the commit needs no such check.)
    fn resolve_book(&self, book: BookRef<'_>) -> Result<BookId, i32> {
        self.reads.path_of(book, |path| {
            if self.in_use(path) {
                return Err(paperpaw_native::host_glue::ERR_BUSY);
            }
            self.edits.book_id(path)
        })
    }

    fn now_ms(&self) -> u64 {
        self.edits.now_ms()
    }

    fn transactions(&self) -> Option<&Transactions> {
        self.edits.transactions()
    }

    fn stage(&self, transaction: u32, item: &StageItem<'_>) -> Result<(), i32> {
        self.edits.stage_item(transaction, item)
    }

    fn abort_staged(&self, transaction: u32) {
        self.edits.abort_staged(transaction);
    }

    fn commit(&self, request: &CommitRequest) -> Result<Committed, i32> {
        let done = self.edits.commit(request)?;
        // The new file is in place (the rename is done): the plugins that
        // subscribed are told, the committer too.
        self.edits.path_of(request.book, &mut |path| {
            self.raise_changed(lib_changed::ANNOTATIONS, path);
        });
        Ok(done)
    }

    fn receipt(
        &self,
        who: &Caller<'_>,
        transaction: u32,
        cursor: u32,
        visit: &mut dyn FnMut(wire::Record<'_>) -> core::ops::ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.edits.receipt(who.owner, transaction, cursor, visit)
    }

    fn receipts_ended(&self, owner: u64) {
        self.edits.receipts_ended(owner);
    }

    fn take_changed(&self, visit: &mut dyn FnMut(u32, u32, &str)) -> bool {
        self.pending_change(visit)
    }

    /// A field is part of the book's details, which a move carries: a book the
    /// job holds is `ERR_BUSY` (as for `provide`).
    fn set_field(&self, book: BookRef<'_>, field: &str, value: &str) -> Result<(), i32> {
        self.reads
            .set_field_unless(book, field, value, &|path| self.in_use(path))?;
        self.raise_changed(lib_changed::FIELDS, "");
        Ok(())
    }
}

impl LibraryHost {
    /// `event.library_changed`'s change, as the pump takes it, with the
    /// catalog's generation (0 where the card cannot tell it now: the plugin
    /// reads again either way).
    fn pending_change(&self, visit: &mut dyn FnMut(u32, u32, &str)) -> bool {
        self.changes.take(&mut |what, path| {
            visit(what, self.reads.generation().unwrap_or(0), path);
        })
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
