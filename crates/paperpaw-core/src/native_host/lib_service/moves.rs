//! The library following a move on root 1 (`files.md` 2.6, `library.md` 6):
//! the journal entry before the rename, the bounded job after it, and what
//! the host does at start.
//!
//! The state machine is here; the card, the catalog and the journal are the
//! host's, behind [`MoveHost`]. For one move the host:
//!
//! 1. [`Moves::begin`]: writes the journal entry (synced), renames, answers.
//!    It does a constant amount of work however many books the folder holds.
//! 2. [`Moves::step`], from the main loop, one bounded step at a time:
//!    carries at most [`STEP_BOOKS`] books (catalogued or not, a `.bak`
//!    sibling too) with position, annotations and plugins' book data, then
//!    the reader assignments, then the folder-valued settings, then sends
//!    the book-moved events, then clears the entry. Every step is idempotent,
//!    so a half-done job is replayed from the entry alone.
//! 3. [`Moves::start`], at start after the FAT repair: reads the open entry;
//!    only `to` exists: finish the job; only `from`: drop the entry (nothing
//!    moved); both: the repair settles them first and the entry is never
//!    dropped blind; neither: drop it (the next walk forgets the books).
//!
//! While the job runs, [`Moves::busy`] says which paths the host answers
//! `ERR_BUSY` for, and [`Moves::open_book`] carries a book a reader opens
//! before the walk reaches it, so it finds its position and annotations.
//!
//! The entry holds `from`, `to` and the `replace` flag. The books below,
//! the reader assignments and the settings are found from `from` and `to`
//! (the old key of a moved path is `from` plus the rest of the path), which
//! keeps the entry, and the call that writes it, bounded.
use alloc::string::String;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};

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

/// What the host backs: the journal, the card and the catalog side.
pub(crate) trait MoveHost {
    /// The open journal entry, if any: calls `visit(from, to, replace)`.
    fn open_entry(&self, visit: &mut dyn FnMut(&str, &str, bool)) -> Result<bool, i32>;
    /// Write the entry and sync it. Nothing else changes.
    fn write_entry(&self, from: &str, to: &str, replace: bool) -> Result<(), i32>;
    /// Remove the entry and sync.
    fn clear_entry(&self) -> Result<(), i32>;
    /// Whether `path` exists on the card (a file or a folder).
    fn exists(&self, path: &str) -> Result<bool, i32>;
    /// The storage's rename (`storage.rename`): the new entry is added before
    /// the old one is removed.
    fn rename(&self, from: &str, to: &str, replace: bool) -> Result<(), i32>;
    /// The FAT repair (`settle_moves`) of a rename a power cut left with both
    /// names.
    fn settle(&self) -> Result<(), i32>;
    /// Whether `path` is `folder` or below it, as the storage names entries
    /// (FAT ignores case: `Books` is `books`).
    fn within(&self, path: &str, folder: &str) -> bool {
        let (path, folder) = (path.trim_matches('/'), folder.trim_matches('/'));
        let n = folder.len();
        path.len() >= n
            && path.is_char_boundary(n)
            && path[..n].eq_ignore_ascii_case(folder)
            && (path.len() == n || path.as_bytes()[n] == b'/')
    }
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
    /// `None`: the entry is open but the state of the card is not decided yet.
    phase: Cell<Option<Phase>>,
    replace: bool,
}

/// The one move in progress, if any. The journal holds one entry, so the next
/// move waits (`ERR_BUSY`) until the job has cleared it.
#[derive(Default)]
pub(crate) struct Moves {
    job: RefCell<Option<Job>>,
}

fn own(text: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}

/// What the card says about an open entry.
enum Verdict {
    /// Only `to` exists: carry the books.
    Finish,
    /// Nothing moved, or nothing is left: the entry is dropped.
    Drop,
    /// Both names are there and the repair did not settle them.
    Undecided,
}

impl Moves {
    /// Move `from` to `to` and start the job: the journal entry, synced,
    /// before the rename; the call answers as soon as the rename has.
    ///
    /// # Errors
    /// `ERR_BUSY` while a job runs; the host's own error from the journal or
    /// the rename (the entry is dropped when nothing moved).
    pub(crate) fn begin(
        &self,
        host: &dyn MoveHost,
        from: &str,
        to: &str,
        replace: bool,
    ) -> Result<(), i32> {
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Err(ERR_BUSY);
        }
        let job = Job {
            from: own(from)?,
            to: own(to)?,
            phase: Cell::new(Some(Phase::Books(0))),
            replace,
        };
        host.write_entry(from, to, replace)?;
        match host.rename(from, to, replace) {
            Ok(()) => {
                *slot = Some(job);
                Ok(())
            }
            Err(error) => {
                // The rename may have been cut short. Look at the card; an
                // entry that cannot be settled now is kept for the next step.
                match Self::decide(host, &job) {
                    Ok(Verdict::Drop) => {}
                    Ok(Verdict::Finish) => *slot = Some(job),
                    Ok(Verdict::Undecided) | Err(_) => {
                        job.phase.set(None);
                        *slot = Some(job);
                    }
                }
                Err(error)
            }
        }
    }

    /// At start, after the FAT repair: pick up an open entry.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, or the host's error reading the journal or the card
    /// (the entry stays and a later `step` decides).
    pub(crate) fn start(&self, host: &dyn MoveHost) -> Result<(), i32> {
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Ok(());
        }
        let mut found = None;
        let mut failed = Ok(());
        let open = host.open_entry(&mut |from, to, replace| {
            found = own(from).and_then(|from| Ok((from, own(to)?))).map_or_else(
                |error| {
                    failed = Err(error);
                    None
                },
                |(from, to)| {
                    Some(Job {
                        from,
                        to,
                        phase: Cell::new(None),
                        replace,
                    })
                },
            );
        })?;
        failed?;
        if open && let Some(job) = found {
            *slot = Some(job);
        }
        drop(slot);
        // Decide at once; an unsettled entry is retried by `step`.
        self.step(host).map(|_| ())
    }

    fn decide(host: &dyn MoveHost, job: &Job) -> Result<Verdict, i32> {
        let mut from = host.exists(&job.from)?;
        let mut to = host.exists(&job.to)?;
        if from && to {
            // A cut between FAT's two directory writes: the repair settles
            // it, and the entry is only judged after.
            host.settle()?;
            from = host.exists(&job.from)?;
            to = host.exists(&job.to)?;
            if from && to {
                // A rename that replaces a file never ran when both names are
                // still there; for anything else the entry stays.
                return if job.replace {
                    host.clear_entry().map(|()| Verdict::Drop)
                } else {
                    Ok(Verdict::Undecided)
                };
            }
        }
        match (from, to) {
            (false, true) => Ok(Verdict::Finish),
            _ => host.clear_entry().map(|()| Verdict::Drop),
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
                    Verdict::Undecided => return Err(ERR_BUSY),
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

    /// Whether `path` is `from` or `to` or below either while the job runs:
    /// a `move`, `storage.rename`, `remove_tree`, `storage.remove` or an
    /// import there answers `ERR_BUSY`.
    pub(crate) fn busy(&self, host: &dyn MoveHost, path: &str) -> bool {
        self.job.try_borrow().map_or(true, |job| {
            job.as_ref()
                .is_some_and(|job| host.within(path, &job.from) || host.within(path, &job.to))
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
            Some(job) if job.phase.get().is_some() && host.within(book, &job.to) => {
                host.carry_book(&job.from, &job.to, book)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "moves/tests.rs"]
mod tests;
