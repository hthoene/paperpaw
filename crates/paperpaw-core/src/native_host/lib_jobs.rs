//! The library's other journalled jobs, a **delete** and an **import**
//! (`library.md` 2.3 and 6): one entry in the move journal
//! (`lib_jobs_journal`), written and synced **before any card change**, then
//! the card's part at once, then the rest in bounded steps of the main loop
//! ([`Jobs::step`], called from the library pump), replayed at start.
//!
//! - **Delete** (≤ 7 books, one entry): the files go (and a `.bak` sibling)
//!   when the call is made, so what the plugin is told per path is the card's
//!   state; then, book by book, the catalog's part (`RemovalCarry`: details,
//!   cover, lists, then position, annotations, fields), what plugins keep
//!   (their `book_data` folders) and the document caches in bounded pieces
//!   (`book_data_steps`), and the `book_removed` event. A file the card
//!   refuses to remove **for good** is left, and so is everything kept for it:
//!   no data is forgotten for a book that is still there. A refusal that may
//!   pass (the card, memory) is repeated until it does. A replay starts again
//!   at the first book: every step is idempotent.
//! - **Import** (one file): the rename into the library (the storage's
//!   journalled move) or, where the two roots are not one card, `ERR_UNSUPPORTED`
//!   (the copy path is not built yet); then the catalog's part
//!   (`ImportCarry`), the documents caches of a replaced book, and
//!   `library_changed`. A cut is decided by the storage's identity of the
//!   file, as a move's is: at `to` it arrived and is catalogued; at `from`
//!   only, nothing happened and the entry is dropped.
//!
//! - **Only the file the entry names is removed.** The entry holds each file's
//!   identity from before it was written; a replay, or a later book's step,
//!   removes a file only while it is still that one. A different file at the
//!   path (copied back, downloaded again) stays; what was kept for the deleted
//!   book goes all the same.
//! - **A transient card error keeps the entry** and the step is retried by the
//!   pump (`Pump::Wait`) for as long as it lasts: every move, delete and import
//!   answers `ERR_BUSY` meanwhile. The entry is never dropped for a card
//!   error (a replay after a restart finishes it); a give-up would leave
//!   deleted books' data behind or drop a half-done import.
//!
//! The journal holds one entry: while any job (this or a move) is open, every
//! `begin` answers `ERR_BUSY`, and the move machine leaves a record of these
//! kinds alone (`Opened::Other`).
use super::{
    book_data_steps::{self, Progress},
    lib_jobs_journal::{Book, DELETE_MAX, JobRecord},
    lib_moves_journal::Journal,
    storage::status,
};
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};
use paperpaw_platform::{Storage, StorageError};

/// Phases of the catalog's side of a delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemovalPhase {
    /// Catalog forget, details file, cover, recent and favourites lists.
    Details,
    /// Position, annotation file, fields, what is kept by the book's path.
    Marks,
    /// All of it is done.
    Done,
}

/// The catalog's side of a delete (Services). Bound to one step of the main
/// loop; every step idempotent, bounded and repeatable after a cut.
pub(crate) trait RemovalCarry {
    /// One bounded step of `phase` for the deleted book `path`: the phase to
    /// run next (`Done` once `Marks` is finished). `with_notes`: annotations
    /// go too. An error (the card, memory) repeats the step; the entry is
    /// never dropped for it. **Except** a status that is final
    /// ([`is_final`]: the card refuses the entry for good, e.g. a damaged
    /// list): the part is left as it is and not repeated. Map card errors with
    /// [`carry_status`] so that this one can be told from a fault that passes.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, the card's status.
    fn forget(
        &self,
        path: &str,
        with_notes: bool,
        phase: RemovalPhase,
    ) -> Result<RemovalPhase, i32>;
}

/// The catalog's side of an import (Services).
pub(crate) trait ImportCarry {
    /// The imported file at `path` is catalogued (a note, the importer's id
    /// kept by path). Idempotent. A final status ([`is_final`], from
    /// [`carry_status`]) is left, not repeated.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, the card's status.
    fn catalogue(&self, path: &str, importer: &str) -> Result<(), i32>;
}

/// What one step of a job needs: the catalog's parts and what the host keeps.
pub(crate) trait JobHost: RemovalCarry + ImportCarry {
    /// The card.
    fn storage(&self) -> &dyn Storage;
    /// One bounded piece of what plugins keep for the book at `path` (and its
    /// `.bak` sibling) and of the host's document caches of both;
    /// `Progress::More`: run again.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, the card's status.
    fn remove_kept(&self, path: &str) -> Result<Progress, i32>;
    /// One bounded piece of the document caches of `path` (an imported file
    /// replaced the book there).
    ///
    /// # Errors
    /// As [`Self::remove_kept`].
    fn remove_cache(&self, path: &str) -> Result<Progress, i32>;
    /// `book_removed` for `path`, at least once.
    ///
    /// # Errors
    /// `ERR_BUSY` when the queue is not free.
    fn removed(&self, path: &str) -> Result<(), i32>;
    /// `library_changed`, at least once.
    ///
    /// # Errors
    /// `ERR_BUSY` when the queue is not free.
    fn changed(&self) -> Result<(), i32>;
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// An entry read at start: the card says what happened to it.
    Decide,
    /// Delete: the files of book `at` are removed.
    Files,
    /// Delete: the catalog's part of book `at`.
    Forget(RemovalPhase),
    /// Delete: what plugins keep and the caches of book `at`.
    Kept,
    /// Delete: `book_removed` for book `at`.
    Event,
    /// `library_changed`, once for the job.
    Changed,
    /// Import: the catalog's note.
    Catalogue,
    /// Import: the replaced book's document caches.
    Cache,
    /// Everything carried: the entry is to be cleared.
    Done,
}

struct Job {
    record: JobRecord,
    phase: Phase,
    /// The book of a delete the phase is at.
    at: usize,
    /// Books whose file the card refuses for good: left as they are.
    skipped: u8,
    /// The books' mask the journal holds (a delete): what is done with and
    /// what is not yet written.
    saved: u8,
}

/// The one delete or import in progress, if any.
#[derive(Default)]
pub(crate) struct Jobs {
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

/// Whether `path` names a folder.
fn is_folder(storage: &dyn Storage, path: &str) -> Result<bool, i32> {
    match storage.list(path, &mut |_| {}) {
        Ok(()) => Ok(true),
        Err(StorageError::NotFound | StorageError::InvalidPath) => Ok(false),
        Err(error) => Err(status(error)),
    }
}

/// The status a catalog part (`RemovalCarry`, `ImportCarry`) answers for a
/// card error: [`status`], except that a damaged entry (`InvalidData`), which
/// `status` calls `ERR_IO` like any card fault, is `ERR_INVALID`, so that
/// [`is_final`] can tell it from a fault that may pass.
pub(crate) fn carry_status(error: StorageError) -> i32 {
    match error {
        StorageError::InvalidData => ERR_INVALID,
        other => status(other),
    }
}

/// Whether a status of a catalog part is for good: the card refuses the entry
/// whatever is tried (`book_data_steps::permanent`, as [`carry_status`] maps
/// it). Such a part is left as it is and never repeated; any other status
/// repeats the step.
pub(crate) const fn is_final(status: i32) -> bool {
    matches!(status, ERR_INVALID | ERR_UNSUPPORTED | ERR_NO_SPACE)
}

/// How removing a file went.
enum Removed {
    /// Gone (or never there).
    Gone,
    /// Left as it is: the card refuses it for good. Nothing is forgotten for
    /// a book that is still there.
    Left,
    /// **Not the file the entry was written for** (a copy put there since):
    /// it is never removed, but the deleted book's data goes all the same
    /// (the user deleted it with its data; the new file must not show the old
    /// position and fields).
    Replaced,
}

/// The storage's identity of what is at `path`: `Ok(Some(0))` nothing is
/// there, `Ok(None)` something is, with no identity (never removed on a
/// guess).
fn found(storage: &dyn Storage, path: &str) -> Result<Option<u64>, i32> {
    match storage.entry_identity(path) {
        Ok(Some(id)) => Ok(Some(id)),
        Ok(None) | Err(StorageError::NotFound) => {
            Ok(if storage.exists(path) { None } else { Some(0) })
        }
        Err(error) => Err(status(error)),
    }
}

/// Removes the file at `path` only while it is the one with identity `id`
/// (0: none was there): a replay after a restart, or a later book's step, must
/// not delete what was put at the path since.
fn remove_file(storage: &dyn Storage, path: &str, id: u64) -> Result<Removed, i32> {
    match found(storage, path)? {
        Some(0) => return Ok(Removed::Gone),
        Some(now) if now == id => {}
        _ => return Ok(Removed::Replaced),
    }
    match storage.remove(path) {
        Ok(()) | Err(StorageError::NotFound) => Ok(Removed::Gone),
        Err(error) if book_data_steps::permanent(error) => Ok(Removed::Left),
        Err(error) => Err(status(error)),
    }
}

/// `<path>.bak`, with fallible allocation.
fn bak_path(path: &str) -> Result<String, i32> {
    let mut bak = String::new();
    bak.try_reserve_exact(path.len() + 4)
        .map_err(|_| ERR_NO_MEMORY)?;
    bak.push_str(path);
    bak.push_str(".bak");
    Ok(bak)
}

/// The book and its `.bak` sibling. The answer is the book's; a `.bak` that
/// is not the entry's or that the card refuses is left (it is no book).
fn remove_files(storage: &dyn Storage, book: &Book) -> Result<Removed, i32> {
    let first = remove_file(storage, &book.path, book.id)?;
    let bak = bak_path(&book.path)?;
    let _ = remove_file(storage, &bak, book.bak)?;
    Ok(first)
}

impl Jobs {
    /// Whether a job is open.
    #[cfg(test)]
    pub(crate) fn active(&self) -> bool {
        self.job.try_borrow().map_or(true, |job| job.is_some())
    }

    /// Whether `path` is a book of a delete that is not carried yet: its file
    /// is gone, its catalog row may not be.
    /// `same` says whether a path of the job names `path` by the storage's own
    /// rules (case, 8.3 aliases).
    pub(crate) fn pending_delete(&self, same: &dyn Fn(&str) -> bool) -> bool {
        self.job.try_borrow().map_or(true, |job| {
            matches!(
                job.as_ref().map(|job| &job.record),
                Some(JobRecord::Delete { books, .. }) if books.iter().any(|b| same(&b.path))
            )
        })
    }

    /// Deletes the books at `paths` (at most 7; the caller has judged they are
    /// books, that no annotations are lost without `with_notes`, and that none
    /// is in use): the entry, synced, then the files. The answer per path
    /// (the first `paths.len()` entries): `OK` the file is gone; another
    /// status the card's, the job repeats it until it passes (a refusal for
    /// good leaves the book as it is).
    ///
    /// # Errors
    /// `ERR_BUSY` while any job or move is open; `ERR_INVALID` for no paths,
    /// more than 7 or a path past the limit; the journal's own error (nothing
    /// changed).
    pub(crate) fn begin_delete(
        &self,
        storage: &dyn Storage,
        paths: &[&str],
        with_notes: bool,
    ) -> Result<[i32; DELETE_MAX], i32> {
        if paths.is_empty() || paths.len() > DELETE_MAX {
            return Err(ERR_INVALID);
        }
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Err(ERR_BUSY);
        }
        // The identities of what is there now, read before anything is
        // written; a book the storage cannot tell is refused (nothing could
        // tell it from another after a cut), one that is gone is carried
        // (only forgotten).
        let mut answers = [0; DELETE_MAX];
        let mut books: Vec<Book> = Vec::new();
        books
            .try_reserve_exact(paths.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        let mut which = [0_usize; DELETE_MAX];
        for (n, path) in paths.iter().enumerate() {
            let path = own(path.trim_matches('/'))?;
            let bak = bak_path(&path)?;
            let (Some(id), Some(bak_id)) = (found(storage, &path)?, found(storage, &bak)?) else {
                answers[n] = ERR_UNSUPPORTED;
                continue;
            };
            which[books.len()] = n;
            books.push(Book {
                path,
                id,
                bak: bak_id,
            });
        }
        if books.is_empty() {
            return Ok(answers);
        }
        let mut record = JobRecord::Delete {
            with_notes,
            books,
            removed: 0,
        };
        // Written, synced, before the first file goes; refused while any
        // entry is there.
        Journal(storage).write_job(&record)?;
        let JobRecord::Delete { books, removed, .. } = &mut record else {
            return Err(ERR_INVALID);
        };
        let mut skipped = 0_u8;
        for (at, book) in books.iter().enumerate() {
            match remove_files(storage, book) {
                Ok(Removed::Gone | Removed::Replaced) => *removed |= 1 << at,
                Ok(Removed::Left) => {
                    skipped |= 1 << at;
                    answers[which[at]] = ERR_IO;
                }
                Err(error) => answers[which[at]] = error,
            }
        }
        // Written again with the books that are done with, so that no replay
        // looks at their paths: on FAT the identity is the first cluster, and
        // a later file may have taken the one the book freed. Best effort
        // here (the answers are due); the step retries it before it goes on.
        let removed = *removed;
        let saved = if Journal(storage).mark_removed(removed).is_ok() {
            removed
        } else {
            0
        };
        *slot = Some(Job {
            record,
            // The files are done; the first step starts the first book's
            // catalog part (a book that failed is retried by its own step).
            phase: Phase::Files,
            at: 0,
            skipped,
            saved,
        });
        Ok(answers)
    }

    /// Brings the file at `from` into the library at `to` (`to` is final:
    /// the name rules and a free name are the caller's). The entry, synced,
    /// then the rename; the catalog follows from the steps.
    ///
    /// # Errors
    /// `ERR_BUSY` while any job or move is open; `ERR_NOT_FOUND` for no
    /// `from`; `ERR_INVALID` for a folder, a name past the limit or `to`
    /// inside `from`; `ERR_STATE` for an existing `to` without `replace` or a
    /// folder there; `ERR_UNSUPPORTED` for a source without an identity or
    /// where the rename cannot cross (no copy yet); the card's status (the
    /// entry is dropped when nothing moved).
    pub(crate) fn begin_import(
        &self,
        storage: &dyn Storage,
        from: &str,
        to: &str,
        importer: &str,
        replace: bool,
    ) -> Result<(), i32> {
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Err(ERR_BUSY);
        }
        let (from, to) = (from.trim_matches('/'), to.trim_matches('/'));
        let identity = match storage.entry_identity(from) {
            Ok(Some(identity)) => identity,
            Ok(None) | Err(StorageError::NotFound) => {
                return Err(if storage.exists(from) {
                    ERR_UNSUPPORTED
                } else {
                    ERR_NOT_FOUND
                });
            }
            Err(error) => return Err(status(error)),
        };
        if is_folder(storage, from)? || from == to {
            return Err(ERR_INVALID);
        }
        if (storage.exists(to) || is_folder(storage, to)?) && (!replace || is_folder(storage, to)?)
        {
            return Err(ERR_STATE);
        }
        let record = JobRecord::Import {
            from: own(from)?,
            to: own(to)?,
            importer: own(importer)?,
            replace,
            identity,
            arrived: false,
        };
        let journal = Journal(storage);
        journal.write_job(&record)?;
        let moved = make_folders(storage, to).and_then(|()| storage.rename(from, to));
        let job = Job {
            record,
            phase: Phase::Catalogue,
            at: 0,
            skipped: 0,
            saved: 0,
        };
        match moved {
            Ok(()) => {
                *slot = Some(job);
                // Best effort: a restart without it decides by identity.
                let _ = journal.mark_arrived();
                Ok(())
            }
            Err(error) => {
                // The rename may have been cut short: look at the card.
                let verdict = decide_import(storage, &job);
                match verdict {
                    Ok(true) => *slot = Some(job),
                    Ok(false) => {
                        // Nothing moved: no entry, so no job. An entry that
                        // cannot be cleared now stays a job in `Decide`: the
                        // next step clears it (no job would block the
                        // journal until a restart).
                        if journal.clear_entry().is_err() {
                            let mut undecided = job;
                            undecided.phase = Phase::Decide;
                            *slot = Some(undecided);
                        }
                    }
                    Err(_) => {
                        let mut undecided = job;
                        undecided.phase = Phase::Decide;
                        *slot = Some(undecided);
                    }
                }
                Err(if error == StorageError::Unsupported {
                    ERR_UNSUPPORTED
                } else {
                    status(error)
                })
            }
        }
    }

    /// At start, after the move machine has read the journal: picks up an
    /// open delete or import (a move's entry, none or one the move machine
    /// sets aside is not for this machine).
    ///
    /// # Errors
    /// The card's status reading the journal (retried by the next call),
    /// `ERR_NO_MEMORY`.
    pub(crate) fn start(&self, storage: &dyn Storage) -> Result<(), i32> {
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if slot.is_some() {
            return Ok(());
        }
        // A move's entry or none: not ours. A record that is not whole was set
        // aside by the move machine before this runs.
        let Some(record) = Journal(storage).read_job()? else {
            return Ok(());
        };
        let phase = match record {
            JobRecord::Delete { .. } => Phase::Files,
            JobRecord::Import { arrived: true, .. } => Phase::Catalogue,
            JobRecord::Import { .. } => Phase::Decide,
        };
        let saved = match &record {
            JobRecord::Delete { removed, .. } => *removed,
            JobRecord::Import { .. } => 0,
        };
        *slot = Some(Job {
            record,
            phase,
            at: 0,
            skipped: 0,
            saved,
        });
        Ok(())
    }
}

/// Makes the folders above `path` that are missing.
fn make_folders(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    let Some((folder, _)) = path.rsplit_once('/') else {
        return Ok(());
    };
    let mut at = 0;
    while let Some(next) = folder.get(at..).and_then(|rest| rest.find('/')) {
        at += next;
        make_folder(storage, &folder[..at])?;
        at += 1;
    }
    make_folder(storage, folder)
}

fn make_folder(storage: &dyn Storage, folder: &str) -> Result<(), StorageError> {
    match storage.list(folder, &mut |_| {}) {
        Ok(()) => Ok(()),
        Err(StorageError::NotFound) => storage.create_dir(folder),
        Err(error) => Err(error),
    }
}

/// By the storage's identity of the imported file: `true` it is at `to`
/// (arrived), `false` it is not.
fn decide_import(storage: &dyn Storage, job: &Job) -> Result<bool, i32> {
    let JobRecord::Import {
        to,
        identity,
        arrived,
        ..
    } = &job.record
    else {
        return Ok(false);
    };
    if *arrived {
        return Ok(true);
    }
    match storage.entry_identity(to) {
        Ok(found) => Ok(found == Some(*identity)),
        Err(StorageError::NotFound) => Ok(false),
        Err(error) => Err(status(error)),
    }
}

impl Jobs {
    /// One bounded step of the job from the main loop; whether more is left.
    /// A failed step keeps the job; the next call repeats it.
    ///
    /// # Errors
    /// The card's, memory's, or the queue's error from the step.
    pub(crate) fn step(&self, host: &dyn JobHost) -> Result<bool, i32> {
        let mut slot = self.job.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let Some(job) = slot.as_mut() else {
            return Ok(false);
        };
        let storage = host.storage();
        match job.phase {
            Phase::Decide => {
                if decide_import(storage, job)? {
                    job.phase = Phase::Catalogue;
                } else {
                    Journal(storage).clear_entry()?;
                    *slot = None;
                    return Ok(false);
                }
            }
            Phase::Done => {
                Journal(storage).clear_entry()?;
                *slot = None;
                return Ok(false);
            }
            _ if matches!(job.record, JobRecord::Delete { .. }) => Self::delete_step(job, host)?,
            _ => Self::import_step(job, host)?,
        }
        Ok(true)
    }

    fn delete_step(job: &mut Job, host: &dyn JobHost) -> Result<(), i32> {
        let JobRecord::Delete {
            with_notes, books, ..
        } = &job.record
        else {
            return Ok(());
        };
        if job.phase == Phase::Changed {
            host.changed()?;
            job.phase = Phase::Done;
            return Ok(());
        }
        let Some(book) = books.get(job.at) else {
            job.phase = Phase::Done;
            return Ok(());
        };
        let path = book.path.as_str();
        job.phase = match job.phase {
            Phase::Files => Self::files_phase(job, host)?,
            Phase::Forget(RemovalPhase::Done) => Phase::Kept,
            Phase::Forget(phase) => match host.forget(path, *with_notes, phase) {
                Ok(next) => Phase::Forget(next),
                // A part the card refuses for good (a damaged list) stays as
                // it is: nothing repeats it, and the rest of the book goes on
                // (the file is gone; its data and its event must follow).
                Err(status) if is_final(status) => Phase::Forget(match phase {
                    RemovalPhase::Details => RemovalPhase::Marks,
                    _ => RemovalPhase::Done,
                }),
                Err(status) => return Err(status),
            },
            Phase::Kept => match host.remove_kept(path)? {
                Progress::More => Phase::Kept,
                Progress::Done => Phase::Event,
            },
            Phase::Event => {
                host.removed(path)?;
                next_book(job)
            }
            _ => Phase::Done,
        };
        Ok(())
    }

    /// The files of book `at`: removed unless the entry says they are done
    /// with, then the books done with are made durable before the catalog's
    /// part starts. A replay removes again only what is not marked; a refusal
    /// for good leaves the book and its data.
    fn files_phase(job: &mut Job, host: &dyn JobHost) -> Result<Phase, i32> {
        let bit = 1_u8 << job.at;
        let JobRecord::Delete { books, removed, .. } = &mut job.record else {
            return Ok(Phase::Done);
        };
        let Some(book) = books.get(job.at) else {
            return Ok(Phase::Done);
        };
        if *removed & bit == 0 {
            match remove_files(host.storage(), book)? {
                Removed::Gone | Removed::Replaced if job.skipped & bit == 0 => *removed |= bit,
                _ => {
                    job.skipped |= bit;
                    return Ok(next_book(job));
                }
            }
        }
        let mask = *removed;
        if job.saved != mask {
            Journal(host.storage()).mark_removed(mask)?;
            job.saved = mask;
        }
        Ok(Phase::Forget(RemovalPhase::Details))
    }

    fn import_step(job: &mut Job, host: &dyn JobHost) -> Result<(), i32> {
        let JobRecord::Import {
            to,
            importer,
            replace,
            ..
        } = &job.record
        else {
            return Ok(());
        };
        job.phase = match job.phase {
            Phase::Catalogue => {
                // The importer's note refused for good (a damaged file) is
                // left: the book is in, and a repeat changes nothing.
                match host.catalogue(to, importer) {
                    Ok(()) => {}
                    Err(status) if is_final(status) => {}
                    Err(status) => return Err(status),
                }
                if *replace {
                    Phase::Cache
                } else {
                    Phase::Changed
                }
            }
            Phase::Cache => match host.remove_cache(to)? {
                Progress::More => Phase::Cache,
                Progress::Done => Phase::Changed,
            },
            Phase::Changed => {
                host.changed()?;
                Phase::Done
            }
            _ => Phase::Done,
        };
        Ok(())
    }
}

/// The phase after a book of a delete: the next book's files, or the end.
fn next_book(job: &mut Job) -> Phase {
    job.at += 1;
    let count = match &job.record {
        JobRecord::Delete { books, .. } => books.len(),
        JobRecord::Import { .. } => 0,
    };
    if job.at < count {
        Phase::Files
    } else {
        // One `library_changed` for the whole delete.
        Phase::Changed
    }
}

#[cfg(test)]
#[path = "lib_jobs_tests.rs"]
mod tests;
