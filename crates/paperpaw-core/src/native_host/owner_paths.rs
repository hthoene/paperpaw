//! What the device's host adapters for `core.net` and `core.sockets` share:
//! the plugin id of an owner (kept after the owner stopped, for its cleanup),
//! a plugin's root-relative path joined for the storage, and the access rules
//! for a write to a root (`files.md` section 3).
use super::{
    host_services::Caller,
    net_service::gate,
    owner_directory::OwnerDirectory,
    shared::Shared,
    staged_name,
    storage::{self, relative_path},
};
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED,
};

/// Longest joined path (as `storage.rs`).
pub(crate) const MAX_JOINED: usize = 640;
/// Root 1 (the user's files) and 2 (the plugin's install folder).
pub(crate) const CARD: u32 = 1;
pub(crate) const BUNDLE: u32 = 2;
/// The folders of root 1 that belong to the kernel and the plugins.
const PROTECTED: [&str; 2] = ["plugins", ".paperpaw"];
/// The host's folder in root 0.
pub(crate) const RECEIVED: &str = ".received";

/// A copy of `text` with room for `extra` more bytes, or `ERR_NO_MEMORY`.
pub(crate) fn owned(text: &str, extra: usize) -> Result<String, i32> {
    gate()?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len() + extra)
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy)
}

/// The owners of one adapter: the kernel's view of them, and the plugin ids
/// kept from their first use to their last cleanup (a stopped owner is gone
/// from the registry before its files are cleaned up).
pub(crate) struct OwnerPaths {
    plugins: RefCell<Vec<(u64, String)>>,
    owners: Shared<OwnerDirectory>,
}

impl OwnerPaths {
    pub(crate) fn new(owners: Shared<OwnerDirectory>) -> Self {
        Self {
            plugins: RefCell::new(Vec::new()),
            owners,
        }
    }

    /// The kernel's view: plugin ids, subscriptions, the screen that shows,
    /// the work run.
    pub(crate) fn owners(&self) -> &Shared<OwnerDirectory> {
        &self.owners
    }

    /// Hands the id of the plugin that owns `owner` to `visit`: the id kept
    /// from an earlier use (a stopped owner is gone from the directory before
    /// its cleanup), else the directory's. `ERR_STATE` when neither knows it.
    pub(crate) fn plugin_of(&self, owner: u64, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        {
            let known = self.plugins.try_borrow().map_err(|_| ERR_BUSY)?;
            if let Some((_, id)) = known.iter().find(|(o, _)| *o == owner) {
                visit(id);
                return Ok(());
            }
        }
        let mut found = false;
        self.owners.with_id(owner, &mut |id| {
            found = true;
            self.remember(owner, id);
            visit(id);
        })?;
        debug_assert!(found);
        Ok(())
    }

    /// Keeps the id of `owner`'s plugin for its cleanup after it stopped. When
    /// there is no room, the id is not kept (the cleanup then needs the view).
    pub(crate) fn remember(&self, owner: u64, id: &str) {
        let Ok(mut known) = self.plugins.try_borrow_mut() else {
            return;
        };
        if known.try_reserve(1).is_err() {
            return;
        }
        if let Ok(copy) = owned(id, 0) {
            known.push((owner, copy));
        }
    }

    /// The owner's last cleanup is done: its id is let go.
    pub(crate) fn forget(&self, owner: u64) {
        if let Ok(mut known) = self.plugins.try_borrow_mut() {
            known.retain(|(o, _)| *o != owner);
        }
    }

    /// `path` of `root` for `plugin`, joined for the storage.
    pub(crate) fn join(plugin: Option<&str>, root: u32, path: &str) -> Result<String, i32> {
        let (kind, id) = match (root, plugin) {
            (CARD, _) => ("", None),
            (0, Some(id)) => (storage::PREFIX, Some(id)),
            (BUNDLE, Some(id)) => ("plugins/", Some(id)),
            _ => return Err(ERR_UNSUPPORTED),
        };
        let id_len = id.map_or(0, |id| id.len() + usize::from(!path.is_empty()));
        let len = kind.len() + id_len + path.len();
        if len > MAX_JOINED {
            return Err(ERR_INVALID);
        }
        gate()?;
        let mut joined = String::new();
        joined
            .try_reserve_exact(len + staged_name::SUFFIX.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        joined.push_str(kind);
        if let Some(id) = id {
            joined.push_str(id);
            if !path.is_empty() {
                joined.push('/');
            }
        }
        joined.push_str(path);
        Ok(joined)
    }

    /// `path` of `root` for the plugin of the call.
    pub(crate) fn join_for(who: &Caller<'_>, root: u32, path: &str) -> Result<String, i32> {
        Self::join(Some(who.plugin), root, path)
    }

    /// `path` of `root` for the plugin that owns `owner`, outside any call.
    pub(crate) fn join_owned(&self, owner: u64, root: u32, path: &str) -> Result<String, i32> {
        if root == CARD {
            return Self::join(None, root, path);
        }
        let mut joined = Err(ERR_UNSUPPORTED);
        self.plugin_of(owner, &mut |id| {
            joined = Self::join(Some(id), root, path);
        })?;
        joined
    }
}

pub(crate) fn access(who: &Caller<'_>, root: u32, path: &str, write: bool) -> Result<(), i32> {
    match root {
        0 => {}
        CARD => who.features.feature("uses.storage")?,
        BUNDLE if write => return Err(ERR_DENIED),
        BUNDLE => {}
        _ => return Err(ERR_UNSUPPORTED),
    }
    if !path.is_empty() {
        relative_path(path.as_bytes())?;
    }
    let first = path.split('/').next().unwrap_or_default();
    if root == CARD && PROTECTED.iter().any(|p| first.eq_ignore_ascii_case(p)) {
        return Err(ERR_INVALID);
    }
    // `.received` is the host's in root 0: a plugin may read what is
    // there, never write into it (`network.md` 5.2).
    if root == 0 && write && first.eq_ignore_ascii_case(RECEIVED) {
        return Err(ERR_DENIED);
    }
    // The staged name is core's, as `.part` was: no path of the plugin's
    // may end in it.
    if write && staged_name::is_staged(path) {
        return Err(ERR_INVALID);
    }
    Ok(())
}
