//! The library's move journal on the card (`library.md` 6, `files.md` 2.6):
//! one record, `.paperpaw/library/journal`, written before the rename of a
//! move and cleared when the job has carried everything.
//!
//! ```text
//! "PPLJ"  version u8 (1)  kind u8 (0 move)  flags u8 (bit 0: replace,
//! bit 1: the replaced file's data is discarded)
//! identity u64 LE
//! from_len u16 LE  to_len u16 LE  from  to  check u64 LE
//! ```
//!
//! `check` is the FNV-1a 64 of every byte before it. The record is written
//! the way every state of the host is: to a temporary file, synced, renamed
//! over the journal, synced. A cut before the rename leaves the old state (no
//! entry, so the move never started: the journal is written before the card
//! changes); a cut after it leaves the new one, never a mix. A record that
//! cannot be read (bad magic, version, length, check, UTF-8) is an error, so
//! the entry is never overwritten blind (`Moves::begin` refuses, the job
//! stays as it is). Phase cursors are not kept: a replay starts from the
//! first phase, and every phase is idempotent. `kind` is 0 for a move; 1
//! (delete) and 2 (import) are reserved for the library's other journalled
//! steps, which reuse the format, and read as an error until they exist.
use super::{lib_service::moves::Entry, storage::status};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_platform::{Storage, StorageError};

/// The journal.
pub(crate) const JOURNAL: &str = ".paperpaw/library/journal";
/// What the record is written to before it replaces the journal.
const TEMP: &str = ".paperpaw/library/journal.tmp";
const MAGIC: &[u8; 4] = b"PPLJ";
const VERSION: u8 = 1;
const HEAD: usize = 4 + 1 + 1 + 1 + 8 + 2 + 2;
/// The flag bits.
const REPLACE: u8 = 1;
const DISCARDED: u8 = 2;
/// The kind of a move (the only one written today).
const KIND_MOVE: u8 = 0;
/// A path of the library, as `core.files` takes it (`MAX_JOINED`).
const MAX_PATH: usize = 640;
/// The most a record holds: the head, two paths and the check.
const MAX_RECORD: usize = HEAD + 2 * MAX_PATH + 8;

/// FNV-1a 64 of `bytes`.
fn check(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn room(bytes: usize) -> Result<Vec<u8>, i32> {
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
    out.push(if entry.replace { REPLACE } else { 0 } | if discarded { DISCARDED } else { 0 });
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
        || flags > REPLACE | DISCARDED
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
        },
        flags & DISCARDED != 0,
    ))
}

/// The journal on `storage`.
pub(crate) struct Journal<'a>(pub(crate) &'a dyn Storage);

impl Journal<'_> {
    /// Visits the open entry; whether there is one.
    ///
    /// # Errors
    /// `ERR_IO` for a record that is not whole (the journal is then never
    /// overwritten); the card's own status for a read that failed;
    /// `ERR_NO_MEMORY`.
    pub(crate) fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        let Some(record) = self.read()? else {
            return Ok(false);
        };
        visit(&decode(&record)?.0);
        Ok(true)
    }

    /// The record, read whole and checked; `None` where there is none.
    fn read(&self) -> Result<Option<Vec<u8>>, i32> {
        let size = match self.0.size(JOURNAL) {
            Ok(size) => size,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(status(error)),
        };
        if size > MAX_RECORD as u64 {
            return Err(ERR_IO);
        }
        let mut record = room(size as usize)?;
        record.resize(size as usize, 0);
        let mut at = 0;
        while at < record.len() {
            let tail = record.get_mut(at..).ok_or(ERR_IO)?;
            match self.0.read_at(JOURNAL, at as u64, tail).map_err(status)? {
                0 => return Err(ERR_IO),
                n => at += n,
            }
        }
        decode(&record)?;
        Ok(Some(record))
    }

    /// Whether the open entry is a replacing move whose replaced file's data
    /// is not discarded yet. No entry: `false`.
    ///
    /// # Errors
    /// `ERR_IO` for a record that is not whole; the card's status.
    pub(crate) fn needs_discard(&self) -> Result<bool, i32> {
        let Some(record) = self.read()? else {
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
        let record = self.read()?.ok_or(ERR_NOT_FOUND)?;
        let (entry, _) = decode(&record)?;
        self.put(&encode(&entry, true)?)
    }

    /// Writes `entry` and syncs it.
    ///
    /// # Errors
    /// The card's status, `ERR_NO_MEMORY`, `ERR_INVALID` for a path past
    /// the limit. The old journal is as it was.
    pub(crate) fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        self.put(&encode(entry, false)?)
    }

    /// The temp, sync, rename, sync of one record.
    fn put(&self, record: &[u8]) -> Result<(), i32> {
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
