//! The library's move journal on the card (`library.md` 6, `files.md` 2.6):
//! one record, `.paperpaw/library/journal`, written before the rename of a
//! move and cleared when the job has carried everything.
//!
//! ```text
//! "PPLJ"  version u8 (1)  kind u8 (0 move)  flags u8 (bit 0: replace,
//! bit 1: the replaced file's data is discarded, bit 2: the rename has
//! succeeded)
//! identity u64 LE
//! from_len u16 LE  to_len u16 LE  from  to  check u64 LE
//! ```
//!
//! `check` is the FNV-1a 64 of every byte before it. The record is written
//! the way every state of the host is: to a temporary file, synced, renamed
//! over the journal, synced. A cut before the rename leaves the old state (no
//! entry, so the move never started: the journal is written before the card
//! changes); a cut after it leaves the new one, never a mix. A record that
//! cannot be read (bad magic, version, length, check, UTF-8) is never
//! overwritten blind and never blocks the library for good: the host sets it
//! aside as `journal.bad` (the one older copy is replaced), counts it for the
//! kernel to log, and moves go on without it ([`Journal::open_or_set_aside`]).
//! A card read that fails is another matter: that stays an error and the
//! journal is left alone. Phase cursors are not kept: a replay starts from the
//! first phase, and every phase is idempotent. `kind` is 0 for a move; 1
//! (delete) and 2 (import) are the library's other journalled steps
//! (`lib_jobs_journal`): the same frame (the 7 bytes up to `flags`, then a
//! kind's own payload, then `check` over everything before it), so one
//! journal holds **one entry of one kind** at a time. A move never writes
//! over another kind's entry (`ERR_BUSY`), and the move machine leaves such an
//! entry alone ([`Opened::Other`]) for the job machine to finish.
//!
//! Interrupted replacing move (`replace` set). The journal is written, then
//! the card renames `from` over the existing `to` (FAT makes that one step
//! whole or not at all), then the first step discards the replaced file's
//! catalog data and records that in the `discarded` flag, then the carry
//! runs. A cut before the rename: the identity is still at `from`, the job is
//! dropped, nothing changed and the replaced file is intact. A cut after the
//! rename: the job is finished from the journal; the replaced file's bytes
//! are gone by then (that is what `replace` asked for). A cut after the
//! discard and before its flag: the discard runs a second time, which is
//! harmless because no book is carried before the flag is written. A cut after
//! the flag: it is never discarded again, so what the carry has put at `to`
//! stays.
use super::{lib_service::moves::Entry, storage::status};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_platform::{Storage, StorageError};

/// The journal.
pub(crate) const JOURNAL: &str = ".paperpaw/library/journal";
/// What the record is written to before it replaces the journal.
const TEMP: &str = ".paperpaw/library/journal.tmp";
/// Where a record that cannot be read is set aside.
pub(crate) const SET_ASIDE: &str = ".paperpaw/library/journal.bad";
pub(super) const MAGIC: &[u8; 4] = b"PPLJ";
pub(super) const VERSION: u8 = 1;
/// The bytes of the frame before a kind's payload: magic, version, kind, flags.
pub(super) const PREFIX: usize = 7;
const HEAD: usize = 4 + 1 + 1 + 1 + 8 + 2 + 2;
/// The flag bits.
const REPLACE: u8 = 1;
const DISCARDED: u8 = 2;
const RENAMED: u8 = 4;
/// The kind of a move.
pub(super) const KIND_MOVE: u8 = 0;
/// The kind of a delete (`lib_jobs_journal`).
pub(super) const KIND_DELETE: u8 = 1;
/// The kind of an import (`lib_jobs_journal`).
pub(super) const KIND_IMPORT: u8 = 2;
/// A path of the library, as `core.files` takes it (`MAX_JOINED`).
const MAX_PATH: usize = 640;
/// The most a record holds: a delete of 7 paths, which is more than a move's
/// head, two paths and the check (read into a buffer of the record's own size,
/// fallible, so the usual record costs what its paths do).
pub(super) const MAX_RECORD: usize = PREFIX + 2 + 7 * (2 + MAX_PATH + 16) + 8;

/// FNV-1a 64 of `bytes`.
pub(super) fn check(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub(super) fn room(bytes: usize) -> Result<Vec<u8>, i32> {
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(bytes).map_err(|_| ERR_NO_MEMORY)?;
    Ok(buffer)
}

/// The record of `entry`.
fn encode(entry: &Entry<'_>, discarded: bool) -> Result<Vec<u8>, i32> {
    let (from, to) = (entry.from.as_bytes(), entry.to.as_bytes());
    if from.len() > MAX_PATH || to.len() > MAX_PATH {
        return Err(ERR_INVALID);
    }
    let mut out = room(HEAD + from.len() + to.len() + 8)?;
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(KIND_MOVE);
    out.push(
        if entry.replace { REPLACE } else { 0 }
            | if discarded { DISCARDED } else { 0 }
            | if entry.renamed { RENAMED } else { 0 },
    );
    out.extend_from_slice(&entry.identity.to_le_bytes());
    // The lengths fit: they are at most `MAX_PATH`.
    out.extend_from_slice(&(from.len() as u16).to_le_bytes());
    out.extend_from_slice(&(to.len() as u16).to_le_bytes());
    out.extend_from_slice(from);
    out.extend_from_slice(to);
    let sum = check(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    Ok(out)
}

/// The kind and flags of a record whose frame is whole (magic, version, a
/// kind this firmware knows, `check`); `ERR_IO` for any other.
pub(super) fn frame(record: &[u8]) -> Result<(u8, u8), i32> {
    if record.len() < PREFIX + 8 || record.get(..4) != Some(&MAGIC[..]) || record[4] != VERSION {
        return Err(ERR_IO);
    }
    let kind = record[5];
    if kind > KIND_IMPORT {
        return Err(ERR_IO);
    }
    let (body, sum) = record.split_at(record.len() - 8);
    if check(body).to_le_bytes() != *sum {
        return Err(ERR_IO);
    }
    Ok((kind, record[6]))
}

/// The entry a record holds; `ERR_IO` for one that is not whole.
fn decode(record: &[u8]) -> Result<(Entry<'_>, bool), i32> {
    let head = record.get(..HEAD).ok_or(ERR_IO)?;
    let word = |at: usize, n: usize| head.get(at..at + n).ok_or(ERR_IO);
    if word(0, 4)? != MAGIC || *head.get(4).ok_or(ERR_IO)? != VERSION {
        return Err(ERR_IO);
    }
    let kind = *head.get(5).ok_or(ERR_IO)?;
    let flags = *head.get(6).ok_or(ERR_IO)?;
    let identity = u64::from_le_bytes(word(7, 8)?.try_into().map_err(|_| ERR_IO)?);
    let from_len = usize::from(u16::from_le_bytes(
        word(15, 2)?.try_into().map_err(|_| ERR_IO)?,
    ));
    let to_len = usize::from(u16::from_le_bytes(
        word(17, 2)?.try_into().map_err(|_| ERR_IO)?,
    ));
    let total = HEAD + from_len + to_len + 8;
    if kind != KIND_MOVE
        || flags & !(REPLACE | DISCARDED | RENAMED) != 0
        || from_len > MAX_PATH
        || to_len > MAX_PATH
        || record.len() != total
    {
        return Err(ERR_IO);
    }
    let (body, sum) = record.split_at(total - 8);
    if check(body).to_le_bytes() != *sum {
        return Err(ERR_IO);
    }
    let from = body.get(HEAD..HEAD + from_len).ok_or(ERR_IO)?;
    let to = body.get(HEAD + from_len..).ok_or(ERR_IO)?;
    Ok((
        Entry {
            from: core::str::from_utf8(from).map_err(|_| ERR_IO)?,
            to: core::str::from_utf8(to).map_err(|_| ERR_IO)?,
            replace: flags & REPLACE != 0,
            identity,
            renamed: flags & RENAMED != 0,
        },
        flags & DISCARDED != 0,
    ))
}

/// The journal on `storage`.
pub(crate) struct Journal<'a>(pub(crate) &'a dyn Storage);

/// Why a record was not read.
pub(super) enum Fault {
    /// The record is not whole.
    Bad,
    /// The card failed (or memory ran out): the journal says nothing.
    Card(i32),
}

impl From<Fault> for i32 {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Bad => ERR_IO,
            Fault::Card(status) => status,
        }
    }
}

/// What [`Journal::open_or_set_aside`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// No journal.
    Nothing,
    /// An entry, visited.
    Entry,
    /// An entry of another kind (a delete or an import): not the moves', not
    /// visited and not set aside; the job machine finishes it.
    Other,
    /// A record that is not whole, now set aside; there is no entry.
    SetAside,
}

impl Journal<'_> {
    /// Visits the open entry; whether there is one.
    ///
    /// # Errors
    /// `ERR_IO` for a record that is not whole (the journal is left as it
    /// is); the card's own status for a read that failed; `ERR_NO_MEMORY`.
    #[cfg(test)]
    pub(crate) fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        let Some(record) = self.read().map_err(i32::from)? else {
            return Ok(false);
        };
        visit(&decode(&record)?.0);
        Ok(true)
    }

    /// Visits the open entry, and sets a record that is not whole aside
    /// (`journal.bad`, replacing an older one) so that moves go on.
    ///
    /// # Errors
    /// The card's status for a read or the set-aside that failed (the
    /// journal is then as it was); `ERR_NO_MEMORY`.
    pub(crate) fn open_or_set_aside(
        &self,
        visit: &mut dyn FnMut(&Entry<'_>),
    ) -> Result<Opened, i32> {
        match self.read() {
            Ok(None) => Ok(Opened::Nothing),
            Ok(Some(record)) => {
                if frame(&record)?.0 != KIND_MOVE {
                    return Ok(Opened::Other);
                }
                visit(&decode(&record)?.0);
                Ok(Opened::Entry)
            }
            Err(Fault::Bad) => {
                self.0.rename(JOURNAL, SET_ASIDE).map_err(status)?;
                self.0.sync().map_err(status)?;
                Ok(Opened::SetAside)
            }
            Err(Fault::Card(error)) => Err(error),
        }
    }

    /// The record, read whole and checked; `None` where there is none.
    pub(super) fn read(&self) -> Result<Option<Vec<u8>>, Fault> {
        let size = match self.0.size(JOURNAL) {
            Ok(size) => size,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(Fault::Card(status(error))),
        };
        if size > MAX_RECORD as u64 {
            return Err(Fault::Bad);
        }
        let mut record = room(size as usize).map_err(Fault::Card)?;
        record.resize(size as usize, 0);
        let mut at = 0;
        while at < record.len() {
            let tail = record.get_mut(at..).ok_or(Fault::Bad)?;
            match self
                .0
                .read_at(JOURNAL, at as u64, tail)
                .map_err(|error| Fault::Card(status(error)))?
            {
                // The card gave less than its size says: that is the card (a
                // short read), not proof the record is bad; it is left alone.
                0 => return Err(Fault::Card(ERR_IO)),
                n => at += n,
            }
        }
        let (kind, _) = frame(&record).map_err(|_| Fault::Bad)?;
        if kind == KIND_MOVE {
            decode(&record).map_err(|_| Fault::Bad)?;
        } else {
            super::lib_jobs_journal::decode(&record).map_err(|_| Fault::Bad)?;
        }
        Ok(Some(record))
    }

    /// Whether the open entry is a replacing move whose replaced file's data
    /// is not discarded yet. No entry: `false`.
    ///
    /// # Errors
    /// `ERR_IO` for a record that is not whole; the card's status.
    pub(crate) fn needs_discard(&self) -> Result<bool, i32> {
        let Some(record) = self.read().map_err(i32::from)? else {
            return Ok(false);
        };
        let (entry, discarded) = decode(&record)?;
        Ok(entry.replace && !discarded)
    }

    /// Records, durably (the entry is written whole again), that the
    /// replaced file's data is discarded: a replay after a cut must not
    /// discard what the carry has put there since.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for no entry; as [`Self::needs_discard`], and the write's.
    pub(crate) fn mark_discarded(&self) -> Result<(), i32> {
        let record = self.read().map_err(i32::from)?.ok_or(ERR_NOT_FOUND)?;
        let (entry, _) = decode(&record)?;
        self.put(&encode(&entry, true)?)
    }

    /// Records, durably (the entry is written whole again), that the rename
    /// has succeeded.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for no entry; as [`Self::needs_discard`], and the write's.
    pub(crate) fn mark_renamed(&self) -> Result<(), i32> {
        let record = self.read().map_err(i32::from)?.ok_or(ERR_NOT_FOUND)?;
        let (entry, discarded) = decode(&record)?;
        self.put(&encode(
            &Entry {
                renamed: true,
                ..entry
            },
            discarded,
        )?)
    }

    /// Writes `entry` and syncs it.
    ///
    /// # Errors
    /// The card's status, `ERR_NO_MEMORY`, `ERR_INVALID` for a path past
    /// the limit. The old journal is as it was.
    pub(crate) fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        // Another kind's entry is never written over (`ERR_BUSY`); a move's
        // own is replaced by its own marks, which use `put` directly.
        if matches!(self.kind()?, Some(kind) if kind != KIND_MOVE) {
            return Err(ERR_BUSY);
        }
        self.put(&encode(entry, false)?)
    }

    /// The kind of the open record: `None` where there is none.
    ///
    /// # Errors
    /// `ERR_IO` for a record that is not whole; the card's status.
    pub(crate) fn kind(&self) -> Result<Option<u8>, i32> {
        match self.read().map_err(i32::from)? {
            Some(record) => Ok(Some(frame(&record)?.0)),
            None => Ok(None),
        }
    }

    /// The temp, sync, rename, sync of one record.
    pub(super) fn put(&self, record: &[u8]) -> Result<(), i32> {
        self.0.write(TEMP, record).map_err(status)?;
        self.0.sync().map_err(status)?;
        self.0.rename(TEMP, JOURNAL).map_err(status)?;
        self.0.sync().map_err(status)
    }

    /// Removes the entry and syncs; no entry is fine.
    ///
    /// # Errors
    /// The card's status.
    pub(crate) fn clear_entry(&self) -> Result<(), i32> {
        match self.0.remove(JOURNAL) {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(error) => return Err(status(error)),
        }
        self.0.sync().map_err(status)
    }
}

#[cfg(test)]
#[path = "lib_moves_journal_tests.rs"]
mod tests;
