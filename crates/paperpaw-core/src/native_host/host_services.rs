//! The host views behind the service families `core.document`, `core.library`
//! and `core.lookup`, kept once per device next to the handle table.
//!
//! A shell (or a test) wires each view once; until then the family answers
//! `ERR_UNSUPPORTED` from its backend. The views are stateless to the
//! dispatcher: the backends get them through [`Caller::hosts`], never through
//! the frame. When an owner is retired, [`HostServices::owner_retired`] ends
//! what it held: its documents (the format is told to close), its image ids
//! and its annotation transactions.
use super::{
    doc_service::{self, DocHost, DocServices},
    lib_service::{self, LibHost, LibServices},
    lookup_service::{LookupHost, LookupServices},
};
use alloc::rc::Rc;
use core::cell::OnceCell;
use paperpaw_native::host_glue::ERR_STATE;

/// What the dispatcher tells a service backend about its caller.
#[derive(Clone, Copy)]
pub(crate) struct Caller<'a> {
    /// The never-reused host owner id.
    pub(crate) owner: u64,
    /// The calling plugin's id.
    pub(crate) plugin: &'a str,
    /// The features the opening instance was admitted with.
    pub(crate) features: super::admitted_features::AdmittedFeatures,
    /// The wired host views.
    pub(crate) hosts: &'a HostServices,
}

#[derive(Default)]
pub(crate) struct HostServices {
    documents: OnceCell<DocServices>,
    library: OnceCell<LibServices>,
    lookup: OnceCell<LookupServices>,
}

impl core::fmt::Debug for HostServices {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostServices")
            .field("documents", &self.documents.get().is_some())
            .field("library", &self.library.get().is_some())
            .field("lookup", &self.lookup.get().is_some())
            .finish()
    }
}

impl HostServices {
    pub(crate) const fn new() -> Self {
        Self {
            documents: OnceCell::new(),
            library: OnceCell::new(),
            lookup: OnceCell::new(),
        }
    }

    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.document` once. `ERR_STATE` when it is wired already.
    pub(crate) fn wire_documents(&self, host: Rc<dyn DocHost>) -> Result<(), i32> {
        self.documents.set(DocServices(host)).map_err(|_| ERR_STATE)
    }
    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.library` once.
    pub(crate) fn wire_library(&self, host: Rc<dyn LibHost>) -> Result<(), i32> {
        self.library.set(LibServices(host)).map_err(|_| ERR_STATE)
    }
    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.lookup` once.
    pub(crate) fn wire_lookup(&self, host: Rc<dyn LookupHost>) -> Result<(), i32> {
        self.lookup.set(LookupServices(host)).map_err(|_| ERR_STATE)
    }

    pub(crate) fn documents(&self) -> Option<&DocServices> {
        self.documents.get()
    }
    pub(crate) fn library(&self) -> Option<&LibServices> {
        self.library.get()
    }
    pub(crate) fn lookup(&self) -> Option<&LookupServices> {
        self.lookup.get()
    }

    /// The owner is retired (its callbacks are revoked and its handles
    /// released). In this order: the documents it opened end, and the format
    /// gets op 4 `close` for each one whose format still runs; then, if the
    /// owner was a format plugin, its documents are closed by the host and
    /// each opener hears `event.document_closed`; then its annotation
    /// transactions end. Safe to call again: the tables are empty then.
    pub(crate) fn owner_retired(&self, owner: u64) {
        if let Some(documents) = self.documents.get() {
            doc_service::opener_stopped(&*documents.0, owner);
            let host = &*documents.0;
            host.formats_of_owner(owner, &mut |format| {
                doc_service::format_stopped(host, format);
            });
        }
        if let Some(library) = self.library.get() {
            lib_service::owner_stopped(&*library.0, owner);
        }
    }
}

#[cfg(test)]
#[path = "host_services_tests.rs"]
mod tests;
