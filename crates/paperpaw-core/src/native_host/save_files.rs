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
//!   request does not remove it, and the host tries the rename again (at the
//!   next claim of that target, by whichever plugin, and when any plugin
//!   starts); only when the target exists again (a newer file) is the kept
//!   one dropped. A kept file whose paths a live claim holds is left alone.
//!   A save to a target whose kept file cannot come back yet is refused
//!   (`ERR_IO`), as is any save once 8 files are kept (`ERR_FULL`).
//!
//! Every allocation is fallible, and made when the claim is: a commit that
//! fails never needs memory to keep the file.
use super::paths_in_use::{PathsInUse, same_name};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_IO, ERR_NO_MEMORY};
use paperpaw_platform::{Storage, StorageError};

/// Kept staged files the table holds at most (a plugin whose renames keep
/// failing is refused a further claim, not given more memory).
const MAX_KEPT: usize = 8;

/// One claimed save.
struct Entry {
    owner: u64,
    /// The root and path the plugin named, so that the claim ends without
    /// building the target again (no memory is needed to let go).
    root: u32,
    rel: String,
    target: String,
    staged: String,
    /// The commit failed with the target gone: the staged file is the only
    /// copy and stays.
    kept: bool,
    /// The claim has ended but the staged file is kept: only the file
    /// remains, waiting for its rename.
    orphan: bool,
}

/// The device's staged files over its storage and its table of paths in use.
pub(crate) struct SaveFiles {
    storage: Rc<dyn Storage>,
    in_use: Rc<PathsInUse>,
    entries: RefCell<Vec<Entry>>,
}

fn same_target(a: &str, b: &str, ignore_case: bool) -> bool {
    if ignore_case { same_name(a, b) } else { a == b }
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
        root: u32,
        rel: &str,
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
        // A staged file kept after a failed rename comes back first, whoever
        // wrote it (the plugin may have restarted, another may save here), so
        // that this save cannot be the one that replaces the only copy. One
        // that cannot come back yet stays, and the save is refused.
        self.recover(Some(target));
        if self.has_kept(target) {
            return Err(ERR_IO);
        }
        if self.kept_count() >= MAX_KEPT {
            self.recover(None);
            if self.kept_count() >= MAX_KEPT {
                return Err(ERR_FULL);
            }
        }
        let entry = Entry {
            owner,
            root,
            rel: copy(rel)?,
            target: copy(target)?,
            staged: copy(staged)?,
            kept: false,
            orphan: false,
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
    #[cfg(test)]
    pub(crate) fn release(&self, owner: u64, target: &str) -> bool {
        self.release_where(|e| e.owner == owner && e.target == target)
    }

    /// The claim `owner` made for `root` and `path` ends, with no allocation:
    /// a stopped plugin's claims go even when the device is out of memory.
    pub(crate) fn release_at(&self, owner: u64, root: u32, path: &str) -> bool {
        self.release_where(|e| e.owner == owner && e.root == root && e.rel == path)
    }

    fn release_where(&self, chosen: impl Fn(&Entry) -> bool) -> bool {
        let Ok(mut entries) = self.entries.try_borrow_mut() else {
            return false;
        };
        let Some(at) = entries.iter().position(|e| !e.orphan && chosen(e)) else {
            return false;
        };
        self.in_use.release(entries[at].owner, &entries[at].target);
        self.in_use.release(entries[at].owner, &entries[at].staged);
        if entries[at].kept {
            // The only copy stays, and so does its record: the rename is
            // tried again.
            entries[at].orphan = true;
        } else {
            let entry = entries.remove(at);
            self.storage.remove(&entry.staged).unwrap_or_default();
        }
        true
    }

    fn kept_count(&self) -> usize {
        self.entries
            .try_borrow()
            .map_or(0, |entries| entries.iter().filter(|e| e.orphan).count())
    }

    /// Whether a kept staged file waits for `target` (under any spelling).
    fn has_kept(&self, target: &str) -> bool {
        let ignore_case = self.storage.ignores_case();
        self.entries.try_borrow().is_ok_and(|entries| {
            entries
                .iter()
                .any(|e| e.orphan && same_target(&e.target, target, ignore_case))
        })
    }

    /// Tries again the rename of the kept staged files for `target` (all:
    /// `None`), whoever kept them: the plugin that wrote one may be gone. A
    /// kept file whose target exists again is stale and goes. One whose
    /// target or staged name is held by a live claim is left alone: that
    /// staged file is a writer's work in progress now.
    pub(crate) fn recover(&self, target: Option<&str>) {
        let Ok(mut entries) = self.entries.try_borrow_mut() else {
            return;
        };
        let ignore_case = self.storage.ignores_case();
        let mut at = 0;
        while at < entries.len() {
            let e = &entries[at];
            let chosen = e.orphan
                && target.is_none_or(|t| same_target(t, &e.target, ignore_case))
                && !self.in_use.holds(&e.target, ignore_case)
                && !self.in_use.holds(&e.staged, ignore_case);
            if !chosen {
                at += 1;
                continue;
            }
            let done = if self.storage.exists(&e.target) {
                self.storage.remove(&e.staged).unwrap_or_default();
                true
            } else {
                self.storage.rename(&e.staged, &e.target).is_ok()
            };
            if done {
                entries.remove(at);
            } else {
                at += 1;
            }
        }
    }

    /// Removes the staged file of `owner`'s claim on `target` (best effort),
    /// unless it was kept; the claim stays.
    pub(crate) fn discard(&self, owner: u64, target: &str) {
        let staged = self.entries.try_borrow().ok().and_then(|entries| {
            entries
                .iter()
                .find(|e| e.owner == owner && e.target == target && !e.kept && !e.orphan)
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
                .find(|e| !e.orphan && e.owner == owner && e.target == target)
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
