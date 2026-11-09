//! The card's side of the library's moves: [`MoveHost`] over the shell's
//! storage, the journal ([`Journal`]) and the shell's identity of entries
//! ([`EntryIdentity`]). What follows a move in the catalog (books' data,
//! reader assignments, folder settings, the book-moved events) is the
//! [`MoveCarry`] the host composes; this file does not know the catalog.
//!
//! - **Identity.** The state machine decides a half-done job by the storage's
//!   identity of the moved entry, never by names. The shell hands over an
//!   [`EntryIdentity`] (FAT: the first cluster; a folder storage: the file
//!   id, no link followed). Where it gives none, every move is refused
//!   (`ERR_UNSUPPORTED`): there is nothing to decide a cut by.
//! - **Same entry.** `Storage::same_entry`; a storage that cannot tell
//!   (`None`) makes the move refuse, since a case-only rename and a move
//!   onto another entry look alike then.
//! - **Settle.** Nothing to do: `paperpaw-fat` settles its interrupted moves
//!   itself before each operation (and at mount), and a folder storage
//!   renames atomically.
//! - **Rename.** `Storage::rename`: the journalled card move
//!   (`library.md` 6), the new entry added before the old one goes.
use super::{
    lib_moves_journal::Journal,
    lib_service::moves::{Entry, MoveHost, Phase},
    storage::status,
};
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_platform::{Storage, StorageError};

/// The storage's identity of an entry, handed over by the shell (a host-only
/// seam: the shared `Storage` trait does not carry it).
pub(crate) trait EntryIdentity {
    /// A number that is the same for an entry before and after a rename, and
    /// different for another entry (FAT: the first cluster; for an empty file
    /// the position of its directory entry, if the rename keeps it). `None`
    /// where the path names nothing or the storage has no stable identity
    /// for it.
    ///
    /// # Errors
    /// The storage's error where it could not be read.
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError>;
}

/// What follows a move in the catalog. Every step is idempotent.
pub(crate) trait MoveCarry {
    /// A replacing move, before the rename: the data kept for the file at
    /// `to` is dropped (see [`MoveHost::discard_replaced`]).
    fn discard_replaced(&self, to: &str) -> Result<(), i32>;
    /// One bounded step of `phase` (see [`MoveHost::carry`]).
    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32>;
    /// One book now (see [`MoveHost::carry_book`]).
    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32>;
}

/// [`MoveHost`] over a storage.
pub(crate) struct CardMoves {
    storage: Rc<dyn Storage>,
    identity: Option<Rc<dyn EntryIdentity>>,
    carry: Rc<dyn MoveCarry>,
}

impl CardMoves {
    pub(crate) fn new(
        storage: Rc<dyn Storage>,
        identity: Option<Rc<dyn EntryIdentity>>,
        carry: Rc<dyn MoveCarry>,
    ) -> Self {
        Self {
            storage,
            identity,
            carry,
        }
    }

    fn journal(&self) -> Journal<'_> {
        Journal(&*self.storage)
    }
}

/// The folder and the last name of `path` (the folder is `""` at the top).
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

impl MoveHost for CardMoves {
    fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        self.journal().open_entry(visit)
    }

    fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        self.journal().write_entry(entry)
    }

    fn clear_entry(&self) -> Result<(), i32> {
        self.journal().clear_entry()
    }

    fn identity(&self, path: &str) -> Result<Option<u64>, i32> {
        let Some(identity) = &self.identity else {
            return Err(ERR_UNSUPPORTED);
        };
        match identity.entry_identity(path) {
            Ok(found) => Ok(found),
            Err(StorageError::NotFound) => Ok(None),
            Err(error) => Err(status(error)),
        }
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

    fn within(&self, path: &str, folder: &str) -> bool {
        folder.is_empty() || super::files_service::within(self.storage.ignores_case(), path, folder)
    }

    fn discard_replaced(&self, to: &str) -> Result<(), i32> {
        self.carry.discard_replaced(to)
    }

    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32> {
        self.carry.carry(from, to, phase)
    }

    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32> {
        self.carry.carry_book(from, to, book)
    }
}

#[cfg(test)]
#[path = "lib_moves_host_tests.rs"]
mod tests;
