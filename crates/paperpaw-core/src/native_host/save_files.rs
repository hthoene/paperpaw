//! The staged files of the host's writers (`core.net` saves, `core.sockets`
//! reads to a file): the claim on the target and its staged file, the writes
//! of the staged file, and the commit that renames it over the target. One
//! path for both services.
//!
//! The commit is made to lose nothing:
//! * the staged file is flushed (`sync`) before the rename;
//! * a rename that fails with the target gone (the storage had taken the old
//!   file away first) leaves the staged file where it is, because it then
//!   holds the only copy of the answer. It is *kept*: the clean-up of the
//!   request does not remove it; the next save to that path replaces it.
//!
//! Every allocation is fallible, and made when the claim is: a commit that
//! fails never needs memory to keep the file.
use super::paths_in_use::PathsInUse;
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::{Storage, StorageError};

/// One claimed save.
struct Entry {
    owner: u64,
    target: String,
    staged: String,
    /// The commit failed with the target gone: the staged file is the only
    /// copy and stays.
    kept: bool,
}

/// The device's staged files over its storage and its table of paths in use.
pub(crate) struct SaveFiles {
    storage: Rc<dyn Storage>,
    in_use: Rc<PathsInUse>,
    entries: RefCell<Vec<Entry>>,
}

fn copy(text: &str) -> Result<String, i32> {
    super::net_service::gate()?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy)
}

impl SaveFiles {
    pub(crate) fn new(storage: Rc<dyn Storage>, in_use: Rc<PathsInUse>) -> Self {
        Self {
            storage,
            in_use,
            entries: RefCell::new(Vec::new()),
        }
    }

    /// `owner` holds `target` and `staged` for a write. `exclusive`: nobody
    /// holds either, `owner` included (`core.sockets`); otherwise `owner`'s
    /// own earlier claim is no obstacle (`core.net` replaces its own request,
    /// the claims are counted).
    ///
    /// # Errors
    /// `ERR_BUSY` when another writer, or (exclusive) the owner, holds either
    /// or the table is borrowed; `ERR_NO_MEMORY`.
    pub(crate) fn claim(
        &self,
        owner: u64,
        target: &str,
        staged: &str,
        exclusive: bool,
    ) -> Result<(), i32> {
        let ignore_case = self.storage.ignores_case();
        let except = (!exclusive).then_some(owner);
        if self.in_use.holds_except(target, ignore_case, except)
            || self.in_use.holds_except(staged, ignore_case, except)
        {
            return Err(ERR_BUSY);
        }
        let entry = Entry {
            owner,
            target: copy(target)?,
            staged: copy(staged)?,
            kept: false,
        };
        let mut entries = self.entries.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.in_use.claim(owner, target)?;
        if let Err(error) = self.in_use.claim(owner, staged) {
            self.in_use.release(owner, target);
            return Err(error);
        }
        entries.push(entry);
        Ok(())
    }

    /// The claim of `owner` on `target` ends; whether it held one. The staged
    /// file is removed, unless it was kept.
    pub(crate) fn release(&self, owner: u64, target: &str) -> bool {
        let Ok(mut entries) = self.entries.try_borrow_mut() else {
            return false;
        };
        let Some(at) = entries
            .iter()
            .position(|e| e.owner == owner && e.target == target)
        else {
            return false;
        };
        let entry = entries.remove(at);
        drop(entries);
        self.in_use.release(owner, &entry.target);
        self.in_use.release(owner, &entry.staged);
        if !entry.kept {
            self.storage.remove(&entry.staged).unwrap_or_default();
        }
        true
    }

    /// Removes the staged file of `owner`'s claim on `target` (best effort),
    /// unless it was kept; the claim stays.
    pub(crate) fn discard(&self, owner: u64, target: &str) {
        let staged = self.entries.try_borrow().ok().and_then(|entries| {
            entries
                .iter()
                .find(|e| e.owner == owner && e.target == target && !e.kept)
                .map(|e| copy(&e.staged))
        });
        if let Some(Ok(staged)) = staged {
            self.storage.remove(&staged).unwrap_or_default();
        }
    }

    /// A new staged file for the claim: an older one goes, an empty one is
    /// made. A kept one is replaced.
    ///
    /// # Errors
    /// The storage's status.
    pub(crate) fn begin(&self, owner: u64, target: &str, staged: &str) -> Result<(), StorageError> {
        self.mark(owner, target, false);
        match self.storage.remove(staged) {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(error) => return Err(error),
        }
        self.storage.write(staged, &[])
    }

    /// Appends `data` to `staged`: whole, or an error.
    ///
    /// # Errors
    /// The storage's status.
    pub(crate) fn append(&self, staged: &str, data: &[u8]) -> Result<(), StorageError> {
        self.storage.append(staged, data)
    }

    /// Flushes `staged` and renames it over `target`. If that fails while
    /// `target` is gone, `staged` is kept, as the only copy.
    ///
    /// # Errors
    /// The storage's status.
    pub(crate) fn commit(
        &self,
        owner: u64,
        target: &str,
        staged: &str,
    ) -> Result<(), StorageError> {
        let done = self
            .storage
            .sync()
            .and_then(|()| self.storage.rename(staged, target));
        if done.is_err() && !self.storage.exists(target) && self.storage.exists(staged) {
            self.mark(owner, target, true);
        }
        done
    }

    fn mark(&self, owner: u64, target: &str, kept: bool) {
        if let Ok(mut entries) = self.entries.try_borrow_mut()
            && let Some(e) = entries
                .iter_mut()
                .find(|e| e.owner == owner && e.target == target)
        {
            e.kept = kept;
        }
    }

    /// Whether the staged file of `owner`'s claim on `target` is kept.
    #[cfg(test)]
    pub(crate) fn is_kept(&self, owner: u64, target: &str) -> bool {
        self.entries
            .borrow()
            .iter()
            .any(|e| e.owner == owner && e.target == target && e.kept)
    }
}

#[cfg(test)]
#[path = "save_files_tests.rs"]
mod tests;
