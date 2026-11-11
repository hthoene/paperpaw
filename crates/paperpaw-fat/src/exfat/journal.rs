//! The write gate of [`ExFatStorage`]: writes only over a verified journal
//! of metadata changes (`PP$INTENT`, see hadris-fat's `bootstrap_intent`,
//! added for PaperPaw); else reads only, with the reason.

use hadris_fs::ErrorKind;
use paperpaw_platform::StorageError;

use super::{ExFatStorage, Result2};
use crate::{Blocks, MountFailure, SharedDisk};

impl<B: Blocks + 'static> ExFatStorage<B> {
    /// [`mount`](Self::mount), writing only once the card's journal is made
    /// or verified (`epoch`: nonzero, new for each card PaperPaw makes one
    /// on, e.g. from a random source). Never fails for the journal: where
    /// it cannot be made the card is mounted again read-only and every
    /// write is refused with [`write_refusal`](Self::write_refusal):
    ///
    /// - [`StorageError::CARD_NEEDS_CHECK`]: the card is marked in use
    ///   (taken from a computer unejected, or a cut, also of an earlier
    ///   try to make the journal) and has no verified journal, or a
    ///   damaged or foreign `PP$INTENT` is there. Not settled: a computer's
    ///   check clears the mark (and frees what a cut try left).
    /// - [`StorageError::Full`]: no room for it, on the card or in a
    ///   block of the root folder's free entries.
    /// - [`StorageError::Io`]: the card failed to be read or written;
    ///   the next mount tries again. A try cut off where it may have left
    ///   something keeps the card marked, so that it is the first case.
    ///
    /// A volume the driver mounts read-only already (a bad up-case table)
    /// is left as [`mount`](Self::mount) leaves it.
    pub fn mount_journaled(disk: SharedDisk<B>, epoch: u64) -> Result<Self, MountFailure> {
        let storage = Self::mount(disk.clone())?;
        if storage.fs.borrow().is_read_only() {
            return Ok(storage);
        }
        // What was changed before is written first: only the setup's own
        // blocks are ever discarded below.
        let refusal = if disk.flush_all().is_err() {
            StorageError::Io
        } else {
            let made = storage.fs.borrow_mut().bootstrap_intent(epoch);
            match made {
                Ok(_) => return Ok(storage),
                Err(e) => match e.kind() {
                    ErrorKind::Corrupt | ErrorKind::Unsupported => StorageError::CARD_NEEDS_CHECK,
                    ErrorKind::NoSpace => StorageError::Full,
                    _ => StorageError::Io,
                },
            }
        };
        // Dropped unsynced, and what the failed try left in the shared
        // block cache forgotten: a read in refused mode would otherwise
        // write it back when it evicts its slot (Senior M1). What reached
        // the card is covered by its in-use mark, which only a computer's
        // check clears.
        drop(storage);
        disk.discard_changes();
        Self::mount_as(disk, Some(refusal))
    }

    /// Why nothing is written to this card, or `None` where it is.
    pub fn write_refusal(&self) -> Option<StorageError> {
        self.refusal
    }

    /// `Ok` where writes are allowed, else why not.
    pub(super) fn writes(&self) -> Result2<()> {
        self.refusal.map_or(Ok(()), Err)
    }
}
