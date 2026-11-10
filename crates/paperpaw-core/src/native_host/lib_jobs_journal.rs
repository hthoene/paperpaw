//! The records of the library's other journalled steps (`library.md` 6), a
//! delete and an import, in the move journal's file and frame
//! (`lib_moves_journal`): `"PPLJ"  version  kind  flags`, a payload, `check`.
//!
//! ```text
//! delete (kind 1)  flags bit 0: with_notes
//!   count u8 (1..=7)  { len u16 LE  path }*count
//! import (kind 2)  flags bit 0: replace, bit 2: the file has arrived
//!   identity u64 LE  from_len u16  to_len u16  importer_len u8
//!   from  to  importer
//! ```
//!
//! Written the way the move's record is: temp, sync, rename over the journal,
//! sync, **before any card change**, and never over another entry (`ERR_BUSY`).
//! Every record is decoded whole or not at all; one that is not (also a kind
//! this firmware does not know) is set aside by the move machine, as before.
use super::lib_moves_journal::{
    Journal, KIND_DELETE, KIND_IMPORT, MAGIC, PREFIX, VERSION, check, frame, room,
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NO_MEMORY};

/// Most paths of one delete (LIB op 10, `library.md` 7.1).
pub(crate) const DELETE_MAX: usize = 7;
/// A path of the library (`MAX_JOINED`).
const MAX_PATH: usize = 640;
/// A plugin id.
const MAX_IMPORTER: usize = 64;
const WITH_NOTES: u8 = 1;
const REPLACE: u8 = 1;
const ARRIVED: u8 = 4;

/// One journalled job, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JobRecord {
    /// Books deleted with everything kept for them.
    Delete {
        with_notes: bool,
        paths: Vec<String>,
    },
    /// A file brought into the library.
    Import {
        from: String,
        to: String,
        importer: String,
        replace: bool,
        /// The storage's identity of `from`, taken before the rename.
        identity: u64,
        /// The rename has succeeded (recorded after it; best effort).
        arrived: bool,
    },
}

fn own(text: &[u8]) -> Result<String, i32> {
    let text = core::str::from_utf8(text).map_err(|_| ERR_IO)?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}

fn le16(bytes: &[u8], at: usize) -> Result<usize, i32> {
    let pair = bytes.get(at..at + 2).ok_or(ERR_IO)?;
    Ok(usize::from(u16::from_le_bytes([pair[0], pair[1]])))
}

/// The record of `job`.
pub(crate) fn encode(job: &JobRecord) -> Result<Vec<u8>, i32> {
    let (kind, flags, payload) = match job {
        JobRecord::Delete { with_notes, paths } => {
            if paths.is_empty() || paths.len() > DELETE_MAX {
                return Err(ERR_INVALID);
            }
            let mut size = 1;
            for path in paths {
                if path.is_empty() || path.len() > MAX_PATH {
                    return Err(ERR_INVALID);
                }
                size += 2 + path.len();
            }
            (KIND_DELETE, u8::from(*with_notes) * WITH_NOTES, size)
        }
        JobRecord::Import {
            from,
            to,
            importer,
            replace,
            arrived,
            ..
        } => {
            if from.is_empty()
                || to.is_empty()
                || from.len() > MAX_PATH
                || to.len() > MAX_PATH
                || importer.is_empty()
                || importer.len() > MAX_IMPORTER
            {
                return Err(ERR_INVALID);
            }
            (
                KIND_IMPORT,
                u8::from(*replace) * REPLACE | u8::from(*arrived) * ARRIVED,
                8 + 2 + 2 + 1 + from.len() + to.len() + importer.len(),
            )
        }
    };
    let mut out = room(PREFIX + payload + 8)?;
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&[VERSION, kind, flags]);
    match job {
        JobRecord::Delete { paths, .. } => {
            // At most `DELETE_MAX`.
            out.push(paths.len() as u8);
            for path in paths {
                out.extend_from_slice(&(path.len() as u16).to_le_bytes());
                out.extend_from_slice(path.as_bytes());
            }
        }
        JobRecord::Import {
            from,
            to,
            importer,
            identity,
            ..
        } => {
            out.extend_from_slice(&identity.to_le_bytes());
            out.extend_from_slice(&(from.len() as u16).to_le_bytes());
            out.extend_from_slice(&(to.len() as u16).to_le_bytes());
            out.push(importer.len() as u8);
            out.extend_from_slice(from.as_bytes());
            out.extend_from_slice(to.as_bytes());
            out.extend_from_slice(importer.as_bytes());
        }
    }
    let sum = check(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    Ok(out)
}

/// The job a record holds (`None` for a move's); `ERR_IO` for one that is not
/// whole.
pub(crate) fn decode(record: &[u8]) -> Result<Option<JobRecord>, i32> {
    let (kind, flags) = frame(record)?;
    let payload = record.get(PREFIX..record.len() - 8).ok_or(ERR_IO)?;
    match kind {
        KIND_DELETE => {
            if flags & !WITH_NOTES != 0 {
                return Err(ERR_IO);
            }
            let count = usize::from(*payload.first().ok_or(ERR_IO)?);
            if count == 0 || count > DELETE_MAX {
                return Err(ERR_IO);
            }
            let mut paths: Vec<String> = Vec::new();
            paths.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
            let mut at = 1;
            for _ in 0..count {
                let len = le16(payload, at)?;
                let bytes = payload.get(at + 2..at + 2 + len).ok_or(ERR_IO)?;
                if len == 0 || len > MAX_PATH {
                    return Err(ERR_IO);
                }
                paths.push(own(bytes)?);
                at += 2 + len;
            }
            if at != payload.len() {
                return Err(ERR_IO);
            }
            Ok(Some(JobRecord::Delete {
                with_notes: flags & WITH_NOTES != 0,
                paths,
            }))
        }
        KIND_IMPORT => {
            if flags & !(REPLACE | ARRIVED) != 0 || payload.len() < 13 {
                return Err(ERR_IO);
            }
            let mut id = [0_u8; 8];
            id.copy_from_slice(payload.get(..8).ok_or(ERR_IO)?);
            let (from_len, to_len) = (le16(payload, 8)?, le16(payload, 10)?);
            let importer_len = usize::from(payload[12]);
            let mut at = 13;
            let mut take = |len: usize| {
                let bytes = payload.get(at..at + len).ok_or(ERR_IO);
                at += len;
                bytes
            };
            let (from, to, importer) = (take(from_len)?, take(to_len)?, take(importer_len)?);
            if at != payload.len()
                || from.is_empty()
                || to.is_empty()
                || importer.is_empty()
                || from_len > MAX_PATH
                || to_len > MAX_PATH
                || importer_len > MAX_IMPORTER
            {
                return Err(ERR_IO);
            }
            Ok(Some(JobRecord::Import {
                from: own(from)?,
                to: own(to)?,
                importer: own(importer)?,
                replace: flags & REPLACE != 0,
                identity: u64::from_le_bytes(id),
                arrived: flags & ARRIVED != 0,
            }))
        }
        _ => Ok(None),
    }
}

impl Journal<'_> {
    /// The open delete or import, if the journal holds one (a move's entry or
    /// none: `None`).
    ///
    /// # Errors
    /// `ERR_IO` for a record that is not whole (the move machine sets it
    /// aside); the card's status; `ERR_NO_MEMORY`.
    pub(crate) fn read_job(&self) -> Result<Option<JobRecord>, i32> {
        match self.read().map_err(i32::from)? {
            Some(record) => decode(&record),
            None => Ok(None),
        }
    }

    /// Writes `job` as the journal's one entry, synced, before the card is
    /// changed.
    ///
    /// # Errors
    /// `ERR_BUSY` while the journal holds any entry (a move's, a job's, one
    /// that cannot be read: it is not written over); `ERR_INVALID` for a
    /// record past the limits; the card's status; `ERR_NO_MEMORY`.
    pub(crate) fn write_job(&self, job: &JobRecord) -> Result<(), i32> {
        if self.kind()?.is_some() {
            return Err(ERR_BUSY);
        }
        // A write that failed may still have reached the card (a failed last
        // sync): the caller is told "no", so the entry is taken away again
        // (best effort; a power cut leaves nobody to tell).
        self.put(&encode(job)?).inspect_err(|_| {
            let _ = self.clear_entry();
        })
    }

    /// Records that the import's file has arrived (the record written whole
    /// again). Best effort for the caller: a restart without it decides by
    /// the storage's identity.
    ///
    /// # Errors
    /// `ERR_STATE` when the open entry is no import; as [`Self::read_job`].
    pub(crate) fn mark_arrived(&self) -> Result<(), i32> {
        match self.read_job()? {
            Some(JobRecord::Import {
                from,
                to,
                importer,
                replace,
                identity,
                ..
            }) => self.put(&encode(&JobRecord::Import {
                from,
                to,
                importer,
                replace,
                identity,
                arrived: true,
            })?),
            _ => Err(paperpaw_native::host_glue::ERR_STATE),
        }
    }
}

#[cfg(test)]
#[path = "lib_jobs_journal_tests.rs"]
mod tests;
