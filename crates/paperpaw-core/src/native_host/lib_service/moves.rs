//! The library following a move on root 1 (`files.md` 2.6, `library.md` 6):
//! the journal entry before the rename, the bounded job after it, and what
//! the host does at start.
//!
//! The state machine is here; the card, the catalog and the journal are the
//! host's, behind [`MoveHost`]. For one move the host:
//!
//! 1. [`Moves::begin`]: refuses a move that cannot succeed (before anything
//!    is written), writes the synced journal entry, drops the data of a file
//!    the move replaces, renames, answers. It does
//!    a constant amount of work however many books the folder holds.
//! 2. [`Moves::step`], from the main loop, one bounded step at a time: carries
//!    at most [`STEP_BOOKS`]
//!    books (catalogued or not, a `.bak` sibling too) with position,
//!    annotations and plugins' book data, then the reader assignments, then
//!    the folder-valued settings, then sends the book-moved events, then
//!    clears the entry. Every step is idempotent, so a half-done job is
//!    replayed from the entry alone.
//! 3. [`Moves::start`], at start after the FAT repair: reads the open entry
//!    and decides by the storage's identity of the moved entry, never by
//!    names: the entry is at `to` (whatever is at `from` now, even a new
//!    file): finish the job; it is at `from` only: drop the entry (nothing
//!    moved); it is nowhere: drop it (the next walk forgets the books). An
//!    entry that is at both names (a cut between FAT's two directory writes)
//!    is settled first. A case-only rename is one entry, so it is decided by
//!    the spelling that is stored. The entry is never left undecided: a job
//!    cannot stay stuck.
//!
//! While the job runs, [`Moves::busy`] says which paths the host answers
//! `ERR_BUSY` for, by [`Access`]: removals and renames at, below or above
//! either name (a folder that holds `from` or `to` cannot go); `make_dir`
//! and imports at or below either name; writes at or below the old name
//! (a new file there as well). Writes below the new name stay allowed: a
//! reader meets the walk there by design. And [`Moves::open_book`] carries a
//! book a reader opens before the walk reaches it, so it finds its position
//! and annotations.
//!
//! The entry holds `from`, `to`, the `replace` flag and the identity of the
//! moved entry. The books below, the reader assignments and the settings are
//! found from `from` and `to` (the old key of a moved path is `from` plus the
//! rest of the path), which keeps the entry, and the call that writes it,
//! bounded.
use alloc::string::String;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE};

/// What an operation does to a path, for [`Moves::busy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    /// Writing or creating a file (also a new file at the old name).
    Write,
    /// `make_dir` or an import: it builds names at or below `from` or `to`.
    Rework,
    /// `storage.remove` or `remove_tree`.
    Remove,
    /// `move` or `storage.rename` (another move waits anyway).
    Rename,
}

/// Books carried by one step.
pub(crate) const STEP_BOOKS: usize = 16;

/// Where the job is. The cursor of a phase only saves work; replaying a phase
/// from its start gives the same state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Books below `from` (and a `.bak` sibling) to their new key.
    Books(u64),
    /// Reader assignments naming `from` or a folder below it.
    Assignments,
    /// Folder-valued settings naming `from` or a folder below it.
    Settings,
    /// The book-moved events.
    Events(u64),
    /// Everything carried; the entry is to be cleared.
    Done,
}

/// The journal entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry<'a> {
    pub(crate) from: &'a str,
    pub(crate) to: &'a str,
    pub(crate) replace: bool,
    /// The storage's identity of the moved entry, taken before the rename.
    pub(crate) identity: u64,
    /// The rename has succeeded (recorded by [`MoveHost::record_renamed`]):
    /// a restart needs no identity to know it.
    pub(crate) renamed: bool,
}

/// What the host backs: the journal, the card and the catalog side.
pub(crate) trait MoveHost {
    /// The open journal entry, if any. `Err` when it cannot be read: the
    /// journal is then not overwritten.
    fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32>;
    /// Write the entry and sync it. Nothing else changes.
    fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32>;
    /// Remove the entry and sync.
    fn clear_entry(&self) -> Result<(), i32>;
    /// The storage's identity of the entry `path` names (FAT: its first
    /// cluster; for an empty file something as stable and as distinct), the
    /// same before and after a rename and different for another entry;
    /// `None` when there is none.
    fn identity(&self, path: &str) -> Result<Option<u64>, i32>;
    /// Whether `path` names a folder.
    fn is_folder(&self, path: &str) -> Result<bool, i32>;
    /// Whether both paths name the same directory entry by the storage's own
    /// rules (case, Unicode case, 8.3 aliases): a case-only rename.
    fn same_entry(&self, a: &str, b: &str) -> Result<bool, i32>;
    /// Whether the stored name of the entry `path` names is spelled exactly
    /// as `path` (the last segment, case included).
    fn spelled_as(&self, path: &str) -> Result<bool, i32>;
    /// The storage's rename (`storage.rename`): the new entry is added before
    /// the old one is removed.
    fn rename(&self, from: &str, to: &str, replace: bool) -> Result<(), i32>;
    /// The FAT repair (`settle_moves`) of a rename a power cut left with both
    /// names.
    fn settle(&self) -> Result<(), i32>;
    /// The rename has succeeded: record it in the entry (written whole
    /// again). Best effort: a restart that does not find the record decides
    /// by the storage's identity, as before; so a failure is not an error
    /// for the move, and a host with no way to record may leave this as is.
    fn record_renamed(&self) -> Result<(), i32> {
        Ok(())
    }
    /// Whether `path` is `folder` or below it, as the storage names entries
    /// (case beyond ASCII, 8.3 aliases). Required: a host that forgets it
    /// would let an alias past the busy rule.
    fn within(&self, path: &str, folder: &str) -> bool;
    /// A replacing move, before the rename: the data kept for the file at `to`
    /// (catalog row, position, annotations, plugins' book data) is dropped so
    /// that the moved book does not inherit it, whether or not the moved book
    /// has data of its own. Idempotent. It runs only here, before the rename:
    /// a replay later would erase what a reader wrote at `to` since.
    fn discard_replaced(&self, to: &str) -> Result<(), i32>;
    /// One bounded step of `phase` for the move `from` -> `to`; returns the
    /// next phase. Idempotent: running a step again after a cut gives the
    /// same state.
    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32>;
    /// Carry one book now (`book` is its path below `to`): what a reader
    /// that opens it during the job needs. Idempotent, and it never replaces
    /// data the book has already at its new key.
    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32>;
}

struct Job {
    from: String,
    to: String,
    identity: u64,
    /// `None`: the entry is open but the state of the card is not decided yet.
    phase: Cell<Option<Phase>>,
    replace: bool,
    /// The entry said the rename had succeeded.
    renamed: bool,
}

/// The one move in progress, if any. The journal holds one entry, so the next
/// move waits (`ERR_BUSY`) until the job has cleared it.
#[derive(Default)]
pub(crate) struct Moves {
    job: RefCell<Option<Job>>,
    /// The journal has been read once; until then no entry is written.
    read: Cell<bool>,
}

pub(crate) fn own(text: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}

/// What the card says about an open entry.
enum Verdict {
    /// The moved entry is at `to`: carry the books.
    Finish,
    /// Nothing moved, or nothing is left: the entry is dropped.
    Drop,
}

impl Moves {
    /// Move `from` to `to` and start the job: refuse what cannot work, the
    /// journal entry, synced, before the rename; the call answers as soon as
    /// the rename has.
    ///
    /// # Errors
    /// `ERR_BUSY` while a job runs; `ERR_NOT_FOUND` for a missing `from`;
    /// `ERR_INVALID` for a move into itself; `ERR_STATE` for an existing `to`
    /// without `replace`, or a folder at `to`, or a folder replacing a file;
    /// the host's own error from the journal or the rename (the entry is
    /// dropped when nothing moved). The journal is not written when the
    /// refusal can be told before.
    pub(crate) fn begin(
        &self,
        host: &dyn MoveHost,
        from: &str,
        to: &str,
        replace: bool,
    ) -> Result<(), i32> {
        // An entry that was never read is never overwritten.
        if !self.read.get() {
            self.start(host)?;
        }
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Err(ERR_BUSY);
        }
        let identity = host.identity(from)?.ok_or(ERR_NOT_FOUND)?;
        // A case-only rename is the one entry under another spelling.
        let same = host.same_entry(from, to)?;
        if !same {
            if host.within(to, from) {
                return Err(ERR_INVALID);
            }
            if host.identity(to)?.is_some()
                && (!replace || host.is_folder(to)? || host.is_folder(from)?)
            {
                return Err(ERR_STATE);
            }
        }
        let job = Job {
            from: own(from)?,
            to: own(to)?,
            identity,
            phase: Cell::new(Some(Phase::Books(0))),
            replace,
            renamed: false,
        };
        host.write_entry(&Entry {
            from,
            to,
            replace,
            identity,
            renamed: false,
        })?;
        // A replacing move drops the replaced file's data before the rename,
        // so the moved book never inherits it (a cut between the two leaves
        // the replaced file without its position; nothing moved).
        let renamed = if replace {
            host.discard_replaced(to)
        } else {
            Ok(())
        }
        .and_then(|()| host.rename(from, to, replace));
        match renamed {
            Ok(()) => {
                *slot = Some(job);
                // Best effort: a restart without the record decides by
                // identity (a cut between the rename and this write).
                let _ = host.record_renamed();
                Ok(())
            }
            Err(error) => {
                // The rename may have been cut short. Look at the card; an
                // entry that cannot be judged now is kept for the next step.
                match Self::decide(host, &job) {
                    Ok(Verdict::Drop) => {}
                    Ok(Verdict::Finish) => *slot = Some(job),
                    Err(_) => {
                        job.phase.set(None);
                        *slot = Some(job);
                    }
                }
                Err(error)
            }
        }
    }

    /// At start, after the FAT repair: pick up an open entry. A journal that
    /// cannot be read leaves the move machinery refusing (`begin` reads it
    /// again first).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, or the host's error reading the journal or the card
    /// (the entry stays and a later `step` decides).
    pub(crate) fn start(&self, host: &dyn MoveHost) -> Result<(), i32> {
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Ok(());
        }
        let mut found = Ok(None);
        let open = host.open_entry(&mut |entry| {
            found = own(entry.from).and_then(|from| {
                Ok(Some(Job {
                    from,
                    to: own(entry.to)?,
                    identity: entry.identity,
                    phase: Cell::new(None),
                    replace: entry.replace,
                    renamed: entry.renamed,
                }))
            });
        })?;
        let found = found?;
        self.read.set(true);
        if open && let Some(job) = found {
            *slot = Some(job);
        }
        drop(slot);
        // Decide at once; a failure is retried by `step`.
        self.step(host).map(|_| ())
    }

    /// By the storage's identity of the moved entry, never by names.
    fn decide(host: &dyn MoveHost, job: &Job) -> Result<Verdict, i32> {
        if job.renamed {
            // The rename succeeded and was recorded: no identity is asked
            // (two names of one file, a twin at `to`, would mislead it).
            host.settle()?;
            return Ok(Verdict::Finish);
        }
        if host.same_entry(&job.from, &job.to)? {
            // A case-only rename: one entry; the stored spelling says whether
            // the rename ran. A cut may have left both spellings; settle.
            return if host.spelled_as(&job.to)? {
                host.settle()?;
                Ok(Verdict::Finish)
            } else {
                host.clear_entry().map(|()| Verdict::Drop)
            };
        }
        let at_to = host.identity(&job.to)? == Some(job.identity);
        let at_from = host.identity(&job.from)? == Some(job.identity);
        if at_to {
            if at_from {
                // A cut between FAT's two directory writes: the entry has both
                // names; the repair settles it.
                host.settle()?;
            }
            // Whatever is at `from` now (even a new file) is not the moved
            // entry: the rename is done.
            Ok(Verdict::Finish)
        } else {
            // At `from` only (nothing moved) or nowhere (it was removed).
            host.clear_entry().map(|()| Verdict::Drop)
        }
    }

    /// One bounded step of the job from the main loop. Returns whether more
    /// is left. A failed step keeps the job; the next call repeats it.
    ///
    /// # Errors
    /// The host's error from the card, the journal or the step.
    pub(crate) fn step(&self, host: &dyn MoveHost) -> Result<bool, i32> {
        {
            let slot = self.job.try_borrow().map_err(|_| ERR_BUSY)?;
            let Some(job) = slot.as_ref() else {
                return Ok(false);
            };
            match job.phase.get() {
                None => match Self::decide(host, job)? {
                    Verdict::Finish => {
                        job.phase.set(Some(Phase::Books(0)));
                        return Ok(true);
                    }
                    Verdict::Drop => {}
                },
                Some(Phase::Done) => host.clear_entry()?,
                Some(phase) => {
                    job.phase.set(Some(host.carry(&job.from, &job.to, phase)?));
                    return Ok(true);
                }
            }
        }
        *self.job.try_borrow_mut().map_err(|_| ERR_BUSY)? = None;
        Ok(false)
    }

    /// Whether a job is open (the entry is not cleared).
    pub(crate) fn active(&self) -> bool {
        self.job.try_borrow().map_or(true, |job| job.is_some())
    }

    /// Whether the host answers `ERR_BUSY` to `access` at `path` while the job
    /// runs (see [`Access`]).
    pub(crate) fn busy(&self, host: &dyn MoveHost, path: &str, access: Access) -> bool {
        self.job.try_borrow().map_or(true, |job| {
            let Some(job) = job.as_ref() else {
                return false;
            };
            let at_or_below = |name: &str| host.within(path, name);
            let above = |name: &str| host.within(name, path);
            match access {
                // A reader and the walk meet below the new name by design
                // (`open_book` carries first): only the old name is closed.
                Access::Write => at_or_below(&job.from),
                Access::Rework => at_or_below(&job.from) || at_or_below(&job.to),
                // A folder that holds either name goes with them.
                Access::Remove | Access::Rename => {
                    at_or_below(&job.from)
                        || at_or_below(&job.to)
                        || above(&job.from)
                        || above(&job.to)
                }
            }
        })
    }

    /// A reader opens `book` (a path below `to`) while the job runs: carry it
    /// first, so its position and annotations are there and nothing the
    /// reader writes is overwritten by the walk later.
    ///
    /// # Errors
    /// The host's error from carrying the book.
    pub(crate) fn open_book(&self, host: &dyn MoveHost, book: &str) -> Result<(), i32> {
        let slot = self.job.try_borrow().map_err(|_| ERR_BUSY)?;
        match slot.as_ref() {
            // Only while the books are carried: once past that phase every
            // book is, and a reader is not held up for the rest of the job.
            Some(job) if host.within(book, &job.to) => match job.phase.get() {
                Some(Phase::Books(_)) => host.carry_book(&job.from, &job.to, book),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "moves/tests.rs"]
mod tests;
