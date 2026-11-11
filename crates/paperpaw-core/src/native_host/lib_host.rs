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
use super::lib_changed::{self, Changes};
use super::lib_cover::{self, CoverCx};
use super::lib_edits::LibEdits;
use super::lib_import::plan;
use super::lib_moves_service::LibraryMoves;
use super::lib_reads::{CatalogEnv, CatalogReads};
use super::lib_service::moves::Access;
use super::lib_service::{
    BookId, BookRef, Caller, CommitRequest, Committed, FieldKind, ImportRequest, LibExcluded,
    LibQuery, LibWrite, One, Rows, StageItem, StoredPosition, Transactions,
};
use super::registrations::{self as wire};
use super::storage_files::{BusySources, excluded_on};
use crate::abi7::context::RowsEnd;
use alloc::rc::Rc;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_NOT_FOUND, ERR_UNSUPPORTED, OK};
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
    /// Every source of paths in use (`core.files`' own): a book open in a
    /// document, the reader's, a transfer's. A delete, an import or a move over
    /// it is `ERR_BUSY`. Without it only the move job counts.
    sources: Option<Rc<BusySources>>,
    /// What changed, for `event.library_changed` (the kernel's pump takes it).
    changes: Changes,
    /// The `cover` calls in a row that answered "the format is not running
    /// yet" and told the caller to ask again (bit 6); at [`COVER_RETRIES`]
    /// it stops telling, so a start that never ends cannot loop a caller
    /// that redraws on the event. A cover made starts it again.
    cover_retries: core::cell::Cell<u8>,
    /// The cover job's failed books (`lib_cover::path_key`, 0: a free place),
    /// the oldest given way to: their op 7 is `ERR_NOT_FOUND` (no cover), never
    /// busy again. Kept in memory only, one array, nothing grows.
    cover_failed: core::cell::Cell<[u64; COVER_FAILED]>,
    cover_failed_next: core::cell::Cell<u8>,
    /// Jobs that failed in a row. The first [`COVER_FAILED`] raise
    /// `library_changed` for their end; after that none does, so a run of
    /// failing books cannot loop a screen that redraws on it. A job that is
    /// ready, or the next start, resets it.
    cover_job_fails: core::cell::Cell<u8>,
    /// What the device's wirings told (the screen's long side for op 7).
    policy: Option<Rc<super::lib_edits::EditPolicy>>,
}

/// How often in a row a busy `cover` raises bit 6 (the format's start is
/// asked for with the first).
const COVER_RETRIES: u8 = 6;

/// How many failed books the host remembers, and how many failed jobs in a row
/// still raise an event.
const COVER_FAILED: usize = 8;

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
            sources: None,
            changes: Changes::default(),
            cover_retries: core::cell::Cell::new(0),
            cover_failed: core::cell::Cell::new([0; COVER_FAILED]),
            cover_failed_next: core::cell::Cell::new(0),
            cover_job_fails: core::cell::Cell::new(0),
            policy: None,
        }
    }

    /// The host's sources of paths in use, asked by `delete`, `import` and
    /// `move_book`.
    pub(crate) fn with_busy_sources(mut self, sources: Rc<BusySources>) -> Self {
        self.sources = Some(sources);
        self
    }

    /// Puts the book with `key` into the failure ring (the oldest gives way).
    fn remember_failed(&self, key: u64) {
        let mut failed = self.cover_failed.get();
        let at = usize::from(self.cover_failed_next.get()) % COVER_FAILED;
        if key != 0 && !failed.contains(&key) {
            failed[at] = key;
            self.cover_failed.set(failed);
            self.cover_failed_next.set(((at + 1) % COVER_FAILED) as u8);
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
        self.edits = self.edits.with_policy(policy.clone());
        self.policy = Some(policy);
        self
    }

    /// Whether the library's move job holds the book at `path`.
    fn in_use(&self, path: &str) -> bool {
        self.busy(path, Access::Rework)
    }

    /// Whether anything holds `path` for `access`: the move job, or (where
    /// wired) a document, the reader or a transfer. For what changes or
    /// removes the file itself.
    fn held(&self, path: &str, access: Access) -> bool {
        self.busy(path, access)
            || self
                .sources
                .as_ref()
                .is_some_and(|all| all.busy(&*self.storage, path, access))
    }

    /// Whether the library's move job holds `path` for `access`.
    fn busy(&self, path: &str, access: Access) -> bool {
        self.moves
            .as_ref()
            .is_some_and(|moves| moves.moves().busy(&**moves.card(), path, access))
    }

    /// Whether the book at `book` is one a delete has removed from the card
    /// and whose catalog part is not done yet: lists and reads leave it out.
    fn deleted(&self, book: BookRef<'_>) -> bool {
        match (book, &self.moves) {
            (BookRef::Path(path), Some(moves)) => moves.pending_delete(path),
            _ => false,
        }
    }

    /// `ERR_NOT_FOUND` for a book a pending delete holds.
    fn visible(&self, book: BookRef<'_>) -> Result<(), i32> {
        if self.deleted(book) {
            Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
        } else {
            Ok(())
        }
    }
}

impl LibQuery for LibraryHost {
    /// As `documents` named it, in whichever wiring call; unsupported (and so
    /// op 7) before.
    fn screen_long_side(&self) -> Result<u32, i32> {
        self.policy
            .as_ref()
            .and_then(|policy| policy.long_side())
            .ok_or(ERR_UNSUPPORTED)
    }
    fn state(&self, visit: One<'_>) -> Result<(), i32> {
        self.reads.state(visit)
    }
    fn query(
        &self,
        query: &wire::ServiceLibQuery<'_>,
        archive: &str,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.reads.query_hiding(
            query,
            archive,
            &|path| self.deleted(BookRef::Path(path)),
            visit,
        )
    }
    fn next_in_series(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.visible(book)?;
        self.reads
            .next_in_series_hiding(book, &|path| self.deleted(BookRef::Path(path)), visit)
    }
    fn fields(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.visible(book)?;
        self.reads.fields(book, cursor, generation, visit)
    }
    fn book(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.visible(book)?;
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
        self.visible(book)?;
        self.reads.text(book, at, visit)
    }
    fn book_key(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.visible(book)?;
        self.reads.book_key(book, visit)
    }
    /// The cover is made by `core.document` from the book's format, in the
    /// caller's frame (`lib_cover`).
    fn cover(
        &self,
        who: &Caller<'_>,
        book: BookRef<'_>,
        side: u32,
        cx: &CoverCx<'_>,
        visit: One<'_>,
    ) -> Result<(), i32> {
        self.visible(book)?;
        // The path is copied out of the catalog read: the format call takes
        // the frame, not a borrow of the catalog.
        let path = self.reads.path_of(book, lib_cover::owned)?;
        // A book whose cover job failed has no cover: not asked again.
        if self
            .cover_failed
            .get()
            .contains(&lib_cover::path_key(&path))
        {
            return Err(ERR_NOT_FOUND);
        }
        match lib_cover::make(cx, who, &path, side, visit) {
            // Preparing (DOCX, MOBI): the book is the cover job, or waits for
            // it; `cover_job_ended` raises bit 6 when the job has ended.
            Ok(lib_cover::Covered::Queued) => Err(ERR_BUSY),
            // The card does not take the job's marker: no cover, and not
            // asked for again (the failure ring), or a screen would loop.
            Ok(lib_cover::Covered::Failed) => {
                self.remember_failed(lib_cover::path_key(&path));
                Err(ERR_NOT_FOUND)
            }
            Ok(lib_cover::Covered::Done) => {
                self.cover_retries.set(0);
                Ok(())
            }
            // A format that is not running yet (its start is requested) is
            // there a moment later: the plugin that was told "busy" is told
            // to ask again (bit 6). Nothing makes the cover in the background
            // in this version.
            Err(ERR_BUSY) => {
                let raised = self.cover_retries.get();
                if raised < COVER_RETRIES {
                    self.cover_retries.set(raised + 1);
                    self.changes.raise(lib_changed::COVER, &path);
                }
                Err(ERR_BUSY)
            }
            // No place in the caller's document table does not clear by
            // itself (the caller closes a document): busy, and no event, or
            // a caller that redraws on it would ask again for ever.
            Err(ERR_FULL) => Err(ERR_BUSY),
            Err(other) => Err(other),
        }
    }
    fn cover_job_left(&self, key: u64) {
        self.remember_failed(key);
    }
    fn cover_jobs_began(&self) {
        self.changes.raise(lib_changed::COVER, "");
    }
    fn cover_job_ended(&self, ready: bool, key: u64) {
        if ready {
            self.cover_job_fails.set(0);
        } else {
            self.remember_failed(key);
            self.cover_job_fails
                .set(self.cover_job_fails.get().saturating_add(1));
        }
        // The screen asks again for what it shows: the book that is ready
        // gets its cover, a failed one is not found, and the next waiting
        // book starts its job. After [`COVER_FAILED`] failed jobs in a row no more
        // is raised.
        if usize::from(self.cover_job_fails.get()) <= COVER_FAILED {
            self.changes.raise(lib_changed::COVER, "");
        }
    }
    fn position(&self, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
        self.visible(book)?;
        self.reads.position(book, visit)
    }
    fn annotations(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.visible(book)?;
        self.reads.annotations(book, cursor, generation, visit)
    }
    fn anchors(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.visible(book)?;
        self.reads.anchors(book, cursor, generation, visit)
    }
}

/// The writes the catalog on the card backs: `provide` (op 24), the positions
/// (ops 13/14's write half), `set_field` (op 23) and `move` (op 11). Each raises its bit of
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

    /// The books at `paths`, as one journalled job (Files'): a path in use
    /// answers `ERR_BUSY` and is not passed on; the rest is, with the card's
    /// answer per path. The catalog's part follows from the pump.
    fn delete(&self, paths: &[&str], with_notes: bool) -> Result<[i32; 7], i32> {
        let Some(moves) = &self.moves else {
            return Err(ERR_UNSUPPORTED);
        };
        let (mut answers, mut asked, mut slots, mut count) = ([OK; 7], [""; 7], [0_usize; 7], 0);
        for (index, path) in paths.iter().enumerate().take(7) {
            if self.held(path, Access::Remove) {
                answers[index] = ERR_BUSY;
            } else {
                asked[count] = path;
                slots[count] = index;
                count += 1;
            }
        }
        if count > 0 {
            let done = moves.begin_delete(&asked[..count], with_notes)?;
            for (slot, status) in slots.iter().zip(done).take(count) {
                answers[*slot] = status;
            }
        }
        Ok(answers)
    }

    fn library_folder(&self, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        self.reads.library_folder(visit);
        Ok(())
    }

    /// An import: the name and the replace rule are decided on the card
    /// ([`plan`]), then the journalled move is Files'; the catalog's part
    /// follows from the pump. A path in use is `ERR_BUSY`.
    fn import(
        &self,
        who: &Caller<'_>,
        request: &ImportRequest<'_>,
        visit: &mut dyn FnMut(&str),
    ) -> Result<(), i32> {
        let Some(moves) = &self.moves else {
            return Err(ERR_UNSUPPORTED);
        };
        let mut planned = Err(ERR_NOT_FOUND);
        self.reads.library_folder(&mut |library| {
            planned = plan(&*self.storage, who.plugin, library, request);
        });
        let planned = planned?;
        if self.held(&planned.to, Access::Rework) {
            return Err(ERR_BUSY);
        }
        moves.begin_import(&planned.from, &planned.to, who.plugin, planned.replace)?;
        visit(&planned.to);
        Ok(())
    }

    /// A move of the book or folder at `from` to `to` (root 1 both), the
    /// card's journalled move (Files'); the catalog, the book's data and the
    /// reader's assignments follow from the pump. A name a document, the
    /// reader, a transfer or the move job holds is `ERR_BUSY`, and nothing is
    /// written then. Without the move job the host has none: `ERR_UNSUPPORTED`.
    fn move_book(&self, from: &str, to: &str, replace: bool) -> Result<(), i32> {
        let Some(moves) = &self.moves else {
            return Err(ERR_UNSUPPORTED);
        };
        let held = |path: &str| {
            self.sources
                .as_ref()
                .is_some_and(|all| all.busy(&*self.storage, path, Access::Rename))
        };
        if held(from) || held(to) {
            return Err(ERR_BUSY);
        }
        moves.begin(from, to, replace)
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
