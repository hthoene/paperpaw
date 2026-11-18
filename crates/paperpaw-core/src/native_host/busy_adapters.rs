//! The busy sources `core.files` waits for besides the network's
//! ([`PathsInUse`](super::paths_in_use::PathsInUse)): the library's move job
//! and the book the reader has open.
//!
//! Neither is registered by `HostResources::wire`: the owner of the state
//! registers it when it exists (the library host with its `Moves`, the kernel
//! when it tracks the reader's book), so that `core.files` is never wired over
//! a state nobody reports. Until both have, `core.files` is refused.
use super::{
    files_service::Access,
    lib_service::moves::{MoveHost, Moves},
    storage_files::BusySource,
};
use alloc::rc::Rc;
use paperpaw_platform::Storage;

/// The library's one move job as a busy source: a path the job holds answers
/// busy as [`Moves::busy`] says, and every `move` is busy while a job is open.
pub(crate) struct MovesBusy {
    moves: Rc<Moves>,
    host: Rc<dyn MoveHost>,
}

impl MovesBusy {
    pub(crate) fn new(moves: Rc<Moves>, host: Rc<dyn MoveHost>) -> Self {
        Self { moves, host }
    }
}

impl BusySource for MovesBusy {
    fn busy(&self, _: &dyn Storage, path: &str, access: Access) -> bool {
        self.moves.busy(&*self.host, path, access)
    }

    fn moving(&self) -> bool {
        self.moves.active()
    }
}

/// The claim owner of the reader's open book in the in-use table. Plugin
/// owners count up from 1, so this one is never a plugin's.
pub(crate) const READER_OWNER: u64 = u64::MAX;
