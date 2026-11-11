//! `core.files` over the shell's storage: the one `FilesHost` every shell
//! uses (`HostWiring::files`), so that the hard parts exist once.
//!
//! The shell gives the card's [`Storage`] and nothing else. Core composes the
//! rest:
//!
//! - `excluded`: the identity of the host's own folders beyond their
//!   spelling. `Storage` has no "same entry" question, so this is the *safe
//!   fallback*: a first segment that carries an 8.3 tail (`~`) and names an
//!   existing entry, in a folder where one of the excluded folders exists, is
//!   refused. It over-refuses (a user's own `MYDOCU~1` next to `.paperpaw` is
//!   refused too) and never lets an alias through; an exact answer waits for a
//!   `Storage` method (Kernel).
//! - `busy` and `moving`: from the registered [`BusySource`]s, the host's own
//!   state (downloads, staged installs, served bodies, the reader's open book,
//!   the library's move job) that the shell cannot know. `HostWiring::files`
//!   refuses (`ERR_STATE`) until every [`BusyKind`] has a source: nothing
//!   busy would be a wrong answer for state the host does hold.
//! - `rename`: the storage's rename; a move of the user's files (root 1) is
//!   the library's move job when it is wired (`with_moves`).
use super::{
    files_service::{Access, FilesHost},
    lib_moves_service::LibraryMoves,
    linked_plugins::LinkedPlugins,
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::Storage;

/// A part of the host that holds paths in use, for `core.files` to answer
/// `ERR_BUSY` about (`files.md` 2).
pub(crate) trait BusySource {
    /// Whether `access` to `path` (card relative) is busy now: the source
    /// holds it, or a folder holding it, in use. `storage` names entries the
    /// way the card does (case beyond ASCII).
    fn busy(&self, storage: &dyn Storage, path: &str, access: Access) -> bool;

    /// Whether a library move job runs (every `move` is busy then).
    fn moving(&self) -> bool {
        false
    }
}

/// What kind of in-use state a [`BusySource`] answers for. `core.files` is not
/// wired until every kind has a source: an unregistered kind would make
/// "nothing is busy" the answer for state the host does hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BusyKind {
    /// The library's move job (`Moves::busy`, and every `move` while it runs).
    LibraryMoves,
    /// `core.net`: a saving request's target and staged file, a served
    /// request's body.
    Network,
    /// The book the reader has open.
    ReaderBook,
}

impl BusyKind {
    const ALL: [Self; 3] = [Self::LibraryMoves, Self::Network, Self::ReaderBook];
}

/// The registered [`BusySource`]s of the device.
#[derive(Default)]
pub(crate) struct BusySources(RefCell<Vec<(BusyKind, Rc<dyn BusySource>)>>);

impl BusySources {
    /// Adds the source of `kind`.
    ///
    /// # Errors
    /// `ERR_BUSY` when the list is in use; `ERR_NO_MEMORY`.
    pub(crate) fn register(&self, kind: BusyKind, source: Rc<dyn BusySource>) -> Result<(), i32> {
        let mut all = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        all.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        all.push((kind, source));
        Ok(())
    }

    /// Whether every kind has a source: what `core.files` needs before it is
    /// wired.
    pub(crate) fn complete(&self) -> bool {
        self.0.try_borrow().is_ok_and(|all| {
            BusyKind::ALL
                .iter()
                .all(|kind| all.iter().any(|(have, _)| have == kind))
        })
    }

    /// Whether a source of `kind` is registered. A list in use answers
    /// "yes" (nothing is added to it then).
    pub(crate) fn has(&self, kind: BusyKind) -> bool {
        self.0
            .try_borrow()
            .map_or(true, |all| all.iter().any(|(have, _)| *have == kind))
    }

    /// A list in use answers busy: the safe answer.
    fn any(&self, ask: impl Fn(&dyn BusySource) -> bool) -> bool {
        self.0
            .try_borrow()
            .map_or(true, |all| all.iter().any(|(_, source)| ask(&**source)))
    }
}

/// The host's folders by root, as `core.files` hides them: `.received` in
/// root 0 (under the plugin's data folder), `plugins` and `.paperpaw` in
/// root 1 (the card's top), and the kernel's `paperpaw.ini` there.
fn excluded_names(root: u32) -> &'static [&'static str] {
    match root {
        0 => &[".received"],
        1 => &["plugins", ".paperpaw", "paperpaw.ini"],
        _ => &[],
    }
}

/// The longest joined `parent/name` the identity check builds: a folder of
/// the host's (at most 128 bytes) and a name of at most 255.
const JOINED: usize = 384;

/// `core.files` over `storage`.
pub(crate) struct StorageFiles {
    storage: Rc<dyn Storage>,
    sources: Rc<BusySources>,
    linked: Rc<LinkedPlugins>,
    /// The library's move job: a move of the user's files goes through it.
    /// `None` where no job is wired (tests): the storage's own rename.
    moves: Option<Rc<LibraryMoves>>,
}

impl StorageFiles {
    pub(crate) fn new(
        storage: Rc<dyn Storage>,
        sources: Rc<BusySources>,
        linked: Rc<LinkedPlugins>,
    ) -> Self {
        Self {
            storage,
            sources,
            linked,
            moves: None,
        }
    }

    /// The same, a move of the user's files started as the library's move
    /// job ([`LibraryMoves::begin`]).
    pub(crate) fn with_moves(mut self, moves: Rc<LibraryMoves>) -> Self {
        self.moves = Some(moves);
        self
    }

    /// Whether the file or folder `parent/name` exists.
    fn exists_in(&self, parent: &str, name: &str) -> bool {
        let mut buf = [0u8; JOINED];
        let need = parent.len() + usize::from(!parent.is_empty()) + name.len();
        let Some(room) = buf.get_mut(..need) else {
            // Too long to be an entry of the card: the safe answer is "yes".
            return true;
        };
        let (head, tail) = room.split_at_mut(parent.len());
        head.copy_from_slice(parent.as_bytes());
        let tail = if parent.is_empty() {
            tail
        } else {
            tail[0] = b'/';
            &mut tail[1..]
        };
        tail.copy_from_slice(name.as_bytes());
        // Both parts are `str` and the `/` is ASCII: the join is UTF-8.
        core::str::from_utf8(room).map_or(true, |path| {
            // A file, or a folder (listing it as far as its first entry).
            self.storage.exists(path) || self.storage.list_from(path, 0, &mut |_| false).is_ok()
        })
    }
}

impl FilesHost for StorageFiles {
    /// A move of the user's files (root 1) is the library's move job: the
    /// journal entry, the rename, then the catalog by the kernel's pump. Any
    /// other move is the storage's rename.
    fn rename(
        &self,
        storage: &dyn Storage,
        from: &str,
        to: &str,
        replace: bool,
        library: bool,
    ) -> Result<(), i32> {
        match &self.moves {
            Some(moves) if library => moves.begin(from, to, replace),
            _ => storage.rename(from, to).map_err(super::storage::status),
        }
    }

    /// A plugin linked into the firmware (`Kernel::register_linked`); a loaded
    /// `.ppn` plugin never is, and a card plugin never takes a linked id.
    fn built_in(&self, plugin: &str) -> bool {
        self.linked.contains(plugin)
    }

    fn excluded(&self, root: u32, parent: &str, first_segment: &str) -> bool {
        let names = excluded_names(root);
        if names
            .iter()
            .any(|name| super::files_service::same_fold(first_segment, name))
        {
            return true;
        }
        // An 8.3 alias carries a `~` (`PAPERP~1`, `RECEIV~1`); without one the
        // spelling decides, and the service has refused it already.
        first_segment.contains('~')
            && self.exists_in(parent, first_segment)
            && names.iter().any(|name| self.exists_in(parent, name))
    }

    fn busy(&self, path: &str, access: Access) -> bool {
        self.sources
            .any(|source| source.busy(&*self.storage, path, access))
    }

    fn moving(&self) -> bool {
        self.sources.any(|source| source.moving())
    }
}

#[cfg(test)]
#[path = "storage_files_tests.rs"]
mod tests;
