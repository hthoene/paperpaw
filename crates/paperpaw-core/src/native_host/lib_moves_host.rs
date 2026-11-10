//! The card's side of the library's moves: [`MoveHost`] over the shell's
//! storage and the journal ([`Journal`]). What follows a move in the catalog
//! (books' data, reader assignments, folder settings, the book-moved events) is the
//! [`MoveCarry`] the host composes; this file does not know the catalog.
//!
//! - **Identity.** The state machine decides a half-done job by the storage's
//!   identity of the moved entry, never by names: `Storage::entry_identity`
//!   (FAT: the first cluster; a folder storage: the file id, no link
//!   followed), `None` for a path that names nothing. A source that is there
//!   without one (the trait default, an empty FAT file) makes the move refuse
//!   (`ERR_UNSUPPORTED`): there is nothing to decide a cut by. Such an entry
//!   at the target is just an existing file (`ERR_STATE`, or replaced when
//!   asked); and after a cut it is simply not the moved entry.
//! - **Same entry.** `Storage::same_entry`; a storage that cannot tell
//!   (`None`) makes the move refuse, since a case-only rename and a move
//!   onto another entry look alike then.
//! - **Settle.** Nothing to do: `paperpaw-fat` settles its interrupted moves
//!   itself before each operation (and at mount), and a folder storage
//!   renames atomically.
//! - **Rename.** `Storage::rename`: the journalled card move
//!   (`library.md` 6), the new entry added before the old one goes.
use super::{
    lib_moves_journal::{Journal, Opened},
    lib_service::moves::{Entry, MoveHost, Phase, own},
    storage::status,
};
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_UNSUPPORTED};
use paperpaw_platform::{Storage, StorageError};

/// What follows a move in the catalog: needs the kernel's view of the
/// catalog (the `Env`), so it is bound to one step of the main loop, never
/// kept. Every step is idempotent.
pub(crate) trait MoveCarry {
    /// The data kept for the file at `to` (catalog row, position,
    /// annotations, plugins' book data) is dropped, so that the moved book
    /// never inherits it; run once per replacing move, before the first book
    /// is carried (see [`StepHost`]). `true` once all of it is gone; `false`:
    /// a bounded piece was done and the step is to run again.
    fn discard(&self, to: &str) -> Result<bool, i32>;
    /// One bounded step of `phase` (see [`MoveHost::carry`]).
    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32>;
    /// One book now (see [`MoveHost::carry_book`]).
    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32>;
}

/// The card's side of the moves, over a storage: the journal, the card
/// rules, the busy rules. Its catalog methods answer `ERR_BUSY` ("not now"):
/// the catalog is bound to a step of the main loop ([`StepHost`]), and a call
/// from a plugin's service call has none. So a plugin's call that starts a
/// move (`begin`) works on the card alone.
///
/// A reader that opens a book below the new name while the job runs
/// (`carry_book`) leaves a one-slot request "carry this book first" and is
/// told to retry (`ERR_BUSY`); the next step serves it ([`StepHost`]) and the
/// retry gets that answer, once. A book the job has already carried costs
/// one retry, not the rest of the job; and once the books are done
/// (`Moves::open_book`) it costs none. A newer request from another book
/// replaces an answer nobody has fetched.
pub(crate) struct CardMoves {
    storage: Rc<dyn Storage>,
    /// Journals set aside as unreadable since the kernel last asked.
    set_aside: Cell<u32>,
    /// The one "carry this book first" request, and its answer.
    request: RefCell<Request>,
    /// Whether the replaced file's data is still to be discarded, kept so
    /// that a step does not read the journal again: `None` until read.
    discard: Cell<Option<bool>>,
}

/// The one request slot of [`CardMoves`].
enum Request {
    Empty,
    /// A reader waits for this book to be carried.
    Asked(String),
    /// The step has carried it (or failed to); the reader's retry fetches this.
    Served(String, Result<(), i32>),
}

impl CardMoves {
    pub(crate) fn new(storage: Rc<dyn Storage>) -> Self {
        Self {
            storage,
            set_aside: Cell::new(0),
            request: RefCell::new(Request::Empty),
            discard: Cell::new(None),
        }
    }

    /// Whether the replaced file's data is still to be discarded (the
    /// journal, read once, then kept).
    fn needs_discard(&self) -> Result<bool, i32> {
        if let Some(pending) = self.discard.get() {
            return Ok(pending);
        }
        let pending = self.journal().needs_discard()?;
        self.discard.set(Some(pending));
        Ok(pending)
    }

    /// Records durably that the data is discarded.
    fn mark_discarded(&self) -> Result<(), i32> {
        self.journal().mark_discarded()?;
        self.discard.set(Some(false));
        Ok(())
    }

    /// A new job or a cleared one: nothing asked or answered is kept.
    fn reset_job(&self, discard: Option<bool>) {
        self.discard.set(discard);
        if let Ok(mut slot) = self.request.try_borrow_mut() {
            *slot = Request::Empty;
        }
    }

    /// Whether two spellings name one path on this card.
    pub(super) fn same_path(&self, a: &str, b: &str) -> bool {
        self.within(a, b) && self.within(b, a)
    }

    fn journal(&self) -> Journal<'_> {
        Journal(&*self.storage)
    }

    /// How many unreadable journals were set aside (`journal.bad`) since the
    /// last call, for the kernel to log: the moves went on without them.
    pub(crate) fn take_set_aside(&self) -> u32 {
        self.set_aside.replace(0)
    }
}

/// The folder and the last name of `path` (the folder is `""` at the top).
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

impl MoveHost for CardMoves {
    fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        // What the journal says is read again.
        self.discard.set(None);
        match self.journal().open_or_set_aside(visit)? {
            Opened::Entry => Ok(true),
            // A delete's or an import's entry: the job machine's; this one has
            // no move (and `write_entry` refuses over it).
            Opened::Nothing | Opened::Other => Ok(false),
            // Not a block for good: the record is kept as `journal.bad`.
            Opened::SetAside => {
                self.set_aside.set(self.set_aside.get().saturating_add(1));
                Ok(false)
            }
        }
    }

    fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        self.journal().write_entry(entry)?;
        self.reset_job(Some(entry.replace));
        Ok(())
    }

    fn clear_entry(&self) -> Result<(), i32> {
        self.journal().clear_entry()?;
        self.reset_job(None);
        Ok(())
    }

    fn identity(&self, path: &str) -> Result<Option<u64>, i32> {
        match self.storage.entry_identity(path) {
            Ok(Some(identity)) => Ok(Some(identity)),
            // No identity (an empty FAT file, a storage that cannot tell, or
            // nothing there): never the moved entry. `begin` refuses a
            // source like that (`exists`), and a decision after a cut treats
            // it as not the entry, so it never fails on it.
            Ok(None) => Ok(None),
            Err(StorageError::NotFound) => Ok(None),
            Err(error) => Err(status(error)),
        }
    }

    fn exists(&self, path: &str) -> Result<bool, i32> {
        Ok(self.storage.exists(path) || self.is_folder(path)?)
    }

    fn is_folder(&self, path: &str) -> Result<bool, i32> {
        match self.storage.list(path, &mut |_| {}) {
            Ok(()) => Ok(true),
            // A file, or nothing, is no folder.
            Err(StorageError::NotFound | StorageError::InvalidPath) => Ok(false),
            Err(error) => Err(status(error)),
        }
    }

    fn same_entry(&self, a: &str, b: &str) -> Result<bool, i32> {
        self.storage.same_entry(a, b).ok_or(ERR_UNSUPPORTED)
    }

    fn spelled_as(&self, path: &str) -> Result<bool, i32> {
        let (folder, name) = split(path);
        let mut spelled = false;
        match self.storage.list(folder, &mut |entry| {
            spelled |= entry.name == name;
        }) {
            Ok(()) => Ok(spelled),
            Err(StorageError::NotFound) => Ok(false),
            Err(error) => Err(status(error)),
        }
    }

    fn rename(&self, from: &str, to: &str, _replace: bool) -> Result<(), i32> {
        // `begin` refused an existing `to` without `replace` already; the
        // storage's rename replaces a file there, which is what `replace`
        // asks for. Nothing but the rename is done here.
        self.storage.rename(from, to).map_err(status)
    }

    fn settle(&self) -> Result<(), i32> {
        Ok(())
    }

    fn record_renamed(&self) -> Result<(), i32> {
        self.journal().mark_renamed()
    }

    fn within(&self, path: &str, folder: &str) -> bool {
        folder.is_empty() || super::files_service::within(self.storage.ignores_case(), path, folder)
    }

    /// Deferred: the catalog is not here. The first step that has it does it
    /// ([`StepHost`]), recorded in the journal so that a replay never
    /// discards what the carry has put there since.
    fn discard_replaced(&self, _to: &str) -> Result<(), i32> {
        Ok(())
    }

    fn carry(&self, _from: &str, _to: &str, _phase: Phase) -> Result<Phase, i32> {
        Err(ERR_BUSY)
    }

    fn carry_book(&self, _from: &str, _to: &str, book: &str) -> Result<(), i32> {
        let mut slot = self.request.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        match &*slot {
            Request::Served(served, answer) if self.same_path(served, book) => {
                let answer = *answer;
                *slot = Request::Empty;
                answer
            }
            // Asked already, by this reader or another: the step has not
            // come yet (one slot).
            Request::Asked(_) => Err(ERR_BUSY),
            Request::Empty | Request::Served(..) => {
                *slot = Request::Asked(own(book)?);
                Err(ERR_BUSY)
            }
        }
    }
}

/// [`CardMoves`] with the catalog of one step: the host `Moves::step` is
/// called with from the main loop.
///
/// A replacing move discards the replaced file's data on the first step, not
/// before the rename (a plugin's call has no catalog): [`Phase::Books`] first
/// asks the journal whether that is still to do, discards, and records it
/// durably, before any book is carried. A reader opening a book meanwhile is
/// told to retry, so it never writes at the key the discard is about to
/// clear; and a replay after a cut sees the record, so it never clears what
/// the carry has put there since.
pub(crate) struct StepHost<'a> {
    card: &'a CardMoves,
    carry: &'a dyn MoveCarry,
}

impl<'a> StepHost<'a> {
    pub(crate) fn new(card: &'a CardMoves, carry: &'a dyn MoveCarry) -> Self {
        Self { card, carry }
    }
}

impl MoveHost for StepHost<'_> {
    fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        self.card.open_entry(visit)
    }
    fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        self.card.write_entry(entry)
    }
    fn clear_entry(&self) -> Result<(), i32> {
        self.card.clear_entry()
    }
    fn identity(&self, path: &str) -> Result<Option<u64>, i32> {
        self.card.identity(path)
    }
    fn exists(&self, path: &str) -> Result<bool, i32> {
        self.card.exists(path)
    }
    fn is_folder(&self, path: &str) -> Result<bool, i32> {
        self.card.is_folder(path)
    }
    fn same_entry(&self, a: &str, b: &str) -> Result<bool, i32> {
        self.card.same_entry(a, b)
    }
    fn spelled_as(&self, path: &str) -> Result<bool, i32> {
        self.card.spelled_as(path)
    }
    fn rename(&self, from: &str, to: &str, replace: bool) -> Result<(), i32> {
        self.card.rename(from, to, replace)
    }
    fn settle(&self) -> Result<(), i32> {
        self.card.settle()
    }
    fn record_renamed(&self) -> Result<(), i32> {
        self.card.record_renamed()
    }
    fn within(&self, path: &str, folder: &str) -> bool {
        self.card.within(path, folder)
    }
    fn discard_replaced(&self, _to: &str) -> Result<(), i32> {
        Ok(())
    }

    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32> {
        if matches!(phase, Phase::Books(_)) && !self.ensure_discarded(to)? {
            // Progress, not a refusal: the same phase again.
            return Ok(phase);
        }
        self.serve(from, to);
        self.carry.carry(from, to, phase)
    }

    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32> {
        // Not before the replaced file's data is discarded.
        if !self.ensure_discarded(to)? {
            return Err(ERR_BUSY);
        }
        self.carry.carry_book(from, to, book)
    }
}

impl StepHost<'_> {
    /// A replacing move's discard, once, recorded before anything is carried.
    /// `false`: a piece of it was done, more is left.
    fn ensure_discarded(&self, to: &str) -> Result<bool, i32> {
        if self.card.needs_discard()? {
            if !self.carry.discard(to)? {
                return Ok(false);
            }
            self.card.mark_discarded()?;
        }
        Ok(true)
    }

    /// The reader's "carry this book first" request, if there is one: the
    /// book is carried now and the answer is left for the reader's retry.
    fn serve(&self, from: &str, to: &str) {
        let Ok(mut slot) = self.card.request.try_borrow_mut() else {
            return;
        };
        let Request::Asked(book) = core::mem::replace(&mut *slot, Request::Empty) else {
            return;
        };
        let answer = match self.ensure_discarded(to) {
            Ok(true) => self.carry.carry_book(from, to, &book),
            Ok(false) => Err(ERR_BUSY),
            Err(error) => Err(error),
        };
        *slot = Request::Served(book, answer);
    }
}

#[cfg(test)]
#[path = "lib_moves_host_tests.rs"]
mod tests;
