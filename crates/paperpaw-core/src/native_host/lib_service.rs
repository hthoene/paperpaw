//! Host side of the service `core.library` (`docs/abi/library.md` 2).
//!
//! Like [`doc_service`](super::doc_service): the backend is the generic
//! stateless function, and the dispatcher supplies the caller and the
//! [`LibHost`] behind the key `core.library`. The ops split in two halves
//! with one trait each: [`query`] (reads: state, query, book, text, facets,
//! series, cover, position, annotations, anchors, fields, book key) and
//! [`write`] (status, delete, move, import, position, staged annotation
//! transactions, fields, provide, receipts). Every op needs the feature
//! `uses.library`, which the generic dispatcher checks against the opening
//! instance's admitted features before this code runs.
use super::lib_cover::CoverCx;
use super::{
    host_services::Caller as HostCaller,
    registrations::{self as wire, Record},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

mod args;
#[expect(
    dead_code,
    reason = "the card and catalog implementation of MoveHost wires it (move-following PR)"
)]
pub(super) mod moves;
mod query;
mod write;
#[allow(unused_imports, reason = "the query and write halves name it")]
pub(crate) use args::BookRef;
#[allow(unused_imports, reason = "the catalog reads implement it")]
pub(crate) use query::{LibQuery, One, Rows};
pub(crate) use write::{BookId, CommitRequest, Committed, StageItem, Transactions};
#[allow(unused_imports, reason = "the catalog's write half implements it")]
pub(crate) use write::{FieldKind, ImportRequest, LibWrite, StoredPosition};

/// The calling plugin: its host owner id and its plugin id.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caller<'a> {
    pub(crate) owner: u64,
    pub(crate) plugin: &'a str,
    /// `uses.storage` (decision 125): the ops that remove or move the user's
    /// files need it besides `uses.library`.
    pub(crate) storage: Result<(), i32>,
}

/// Everything the library service needs from the host's catalog, card and
/// clock. The shell implements it; the tests use fakes.
pub(crate) trait LibHost: query::LibQuery + write::LibWrite + LibExcluded {}
impl<T: query::LibQuery + write::LibWrite + LibExcluded> LibHost for T {}

/// The card folders root 1 never reaches (`plugins`, `.paperpaw`), as the host
/// resolves them. The service already refuses their spelling in any case.
pub(crate) trait LibExcluded {
    /// Whether the path's first segment names one of them by another name,
    /// as FAT finds an entry (an 8.3 short alias).
    ///
    /// Required: a host that forgets it would let an alias reach the
    /// protected folders.
    fn excluded(&self, first_segment: &str) -> bool;
}

/// What the shell hands the frame under the key `core.library`.
pub(crate) struct LibServices(pub(crate) Rc<dyn LibHost>);

/// The plugin stopped: its annotation transactions end and what they staged
/// is dropped. The table is released before the host is called.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed; a retry ends them.
pub(crate) fn owner_stopped(host: &dyn LibHost, owner: u64) -> Result<(), i32> {
    let closed = host
        .transactions()
        .map_or(Ok(()), |t| t.close_owner(owner, |id| host.abort_staged(id)));
    host.receipts_ended(owner);
    closed
}

/// The `core.library` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    caller: &HostCaller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = caller.hosts.library().ok_or(ERR_UNSUPPORTED)?;
    let who = Caller {
        owner: caller.owner,
        plugin: caller.plugin,
        storage: caller.features.feature("uses.storage"),
    };
    let host: &dyn LibHost = &*services.0;
    match op.id {
        1..=7 | 13 | 15 | 16 | 22 | 25 => {
            let cx = CoverCx { frame, caller };
            query::run(host, &who, &cx, op, input, out)
        }
        8..=12 | 14 | 17..=21 | 23 | 24 | 26 => write::run(host, &who, op, input, out),
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_one(record, out)
}

#[cfg(test)]
#[path = "lib_service/tests.rs"]
mod tests;
