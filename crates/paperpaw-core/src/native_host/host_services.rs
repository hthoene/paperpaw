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
    secrets_service::SecretsServices,
    work_service::WorkServices,
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
    work: OnceCell<WorkServices>,
    secrets: OnceCell<SecretsServices>,
}

impl core::fmt::Debug for HostServices {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostServices")
            .field("documents", &self.documents.get().is_some())
            .field("library", &self.library.get().is_some())
            .field("lookup", &self.lookup.get().is_some())
            .field("work", &self.work.get().is_some())
            .field("secrets", &self.secrets.get().is_some())
            .finish()
    }
}

impl HostServices {
    pub(crate) const fn new() -> Self {
        Self {
            documents: OnceCell::new(),
            library: OnceCell::new(),
            lookup: OnceCell::new(),
            work: OnceCell::new(),
            secrets: OnceCell::new(),
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

    #[allow(dead_code, reason = "a shell wires its scheduler; tests do today")]
    /// Wire `core.work` once.
    pub(crate) fn wire_work(&self, work: WorkServices) -> Result<(), i32> {
        self.work.set(work).map_err(|_| ERR_STATE)
    }

    pub(crate) fn work(&self) -> Option<&WorkServices> {
        self.work.get()
    }

    #[allow(dead_code, reason = "a shell wires its secrets host; tests do today")]
    /// Wire `core.secrets` once.
    pub(crate) fn wire_secrets(&self, secrets: SecretsServices) -> Result<(), i32> {
        self.secrets.set(secrets).map_err(|_| ERR_STATE)
    }

    pub(crate) fn secrets(&self) -> Option<&SecretsServices> {
        self.secrets.get()
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
    /// released). In this order: if the owner was a format plugin, every
    /// document of its formats is marked gone and closed by the host, and each
    /// other opener hears `event.document_closed` once (the retiring owner
    /// hears nothing); then the documents it opened end, and the format gets
    /// op 4 `close` for each one whose format still runs (never one of the
    /// retiring plugin's own); then its annotation transactions end. Safe to
    /// call again: the tables are empty then.
    ///
    /// Every step runs; the first failure is returned. `ERR_BUSY` (a table is
    /// borrowed) means a retry finishes what is left.
    pub(crate) fn owner_retired(&self, owner: u64) -> Result<(), i32> {
        let mut result = Ok(());
        if let Some(documents) = self.documents.get() {
            let host = &*documents.0;
            host.formats_of_owner(owner, &mut |format| {
                result =
                    result.and(doc_service::format_retired(host, format, Some(owner)).map(|_| ()));
            });
            result = result.and(doc_service::opener_stopped(host, owner).map(|_| ()));
        }
        if let Some(library) = self.library.get() {
            result = result.and(lib_service::owner_stopped(&*library.0, owner));
        }
        // Last: whatever the owner declared as work is gone (idempotent).
        if let Some(work) = self.work.get() {
            result = result.and(work.sched.owner_stopped(owner));
        }
        result
    }
}

#[cfg(test)]
#[path = "host_services_tests.rs"]
pub(crate) mod tests;
