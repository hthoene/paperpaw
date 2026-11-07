//! Host side of the service `core.library` (`docs/abi/library.md` 2).
//!
//! Like [`doc_service`](super::doc_service): the backend is the generic
//! stateless function, and the frame supplies the caller and the
//! [`LibHost`] behind the key `core.library`. The ops split in two halves
//! with one trait each: [`query`] (reads: state, query, book, text, facets,
//! series, cover, position, annotations, anchors, fields, book key) and
//! [`write`] (status, delete, move, import, position, staged annotation
//! transactions, fields, provide, receipts). Every op needs the feature
//! `uses.library`, which the generic dispatcher checks through
//! `Frame::feature` before this code runs.
use super::registrations::{self as wire, Record};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

mod args;
mod query;
mod write;
#[allow(unused_imports, reason = "the query and write halves name it")]
pub(crate) use args::BookRef;

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

/// The `core.library` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = frame
        .host_service("core.library")
        .and_then(<dyn core::any::Any>::downcast_ref::<LibServices>)
        .ok_or(ERR_UNSUPPORTED)?;
    let who = Caller {
        owner: frame.caller()?,
        plugin: frame.caller_plugin()?,
        storage: frame.feature("uses.storage"),
    };
    let host: &dyn LibHost = &*services.0;
    match op.id {
        1..=7 | 13 | 15 | 16 | 22 | 25 => query::run(host, &who, op, input, out),
        8..=12 | 14 | 17..=21 | 23 | 24 | 26 => write::run(host, &who, op, input, out),
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn encode(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    wire::record_encode_one(record, out)
}

#[cfg(test)]
#[path = "lib_service/tests.rs"]
mod tests;
