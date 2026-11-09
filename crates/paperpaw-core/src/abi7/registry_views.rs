//! What `core.registry` reads, as a frame offers it (`registry.md` section 2).
//! The rows leave the view as records that encode themselves, so this file
//! needs no record types: the host side
//! (`native_host::registry_service`) implements [`Row`] for its records.
#![allow(
    dead_code,
    reason = "the frame tests that include this file through `context.rs` read none of it"
)]
use core::ops::ControlFlow;

/// One reply record, ready to be framed into a reply buffer.
pub(crate) trait Row {
    /// Its framed length in bytes.
    fn framed_len(&self) -> Result<usize, i32>;
    /// Frames it at the front of `out`; the length written.
    fn encode(&self, out: &mut [u8]) -> Result<usize, i32>;
}

/// Who asks: what the rows that depend on the caller (values it may read)
/// are decided by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Asker<'a> {
    /// The never-reused host owner id.
    pub(crate) owner: u64,
    /// The calling plugin's id.
    pub(crate) plugin: &'a str,
}

/// The host's tables as `core.registry` reads them. Every list is walked in
/// one fixed order, so a cursor is an index into it.
pub(crate) trait RegistryView {
    /// The registry generation: it changes when the set or order of a list
    /// changes, and is never 0.
    fn generation(&self) -> u32;

    /// The rows of list op `op` for `scope`, in order, until `visit` breaks.
    /// The rows are built for `visit` alone: none outlives the call. A scope
    /// that names nothing known visits nothing. `ERR_UNSUPPORTED` for an op
    /// this view does not carry.
    fn rows(
        &self,
        op: u32,
        scope: &str,
        asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
    ) -> Result<(), i32>;

    /// The records of detail op `op` for `key`, in order. `ERR_NOT_FOUND` for
    /// a key that is not there (no record visited).
    fn detail(
        &self,
        op: u32,
        key: &str,
        asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
    ) -> Result<(), i32>;
}
