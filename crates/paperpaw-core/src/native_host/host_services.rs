//! The host views behind the service families `core.document`, `core.files`,
//! `core.library` and `core.lookup`, kept once per device next to the handle table.
//!
//! A shell (or a test) wires each view once; until then the family answers
//! `ERR_UNSUPPORTED` from its backend. The views are stateless to the
//! dispatcher: the backends get them through [`Caller::hosts`], never through
//! the frame. When an owner is retired, [`HostServices::owner_retired`] ends
//! what it held: its documents (the format is told to close), its image ids
//! and its annotation transactions.
use super::{
    doc_service::{self, DocHost, DocServices},
    files_service::{FilesHost, FilesServices},
    hyphen_service::HyphenServices,
    lib_service::{self, LibHost, LibServices},
    lookup_service::{LookupHost, LookupServices},
    net_service::{self, NetHost, NetServices},
    secrets_service::SecretsServices,
    ui_service::UiServices,
    work_service::WorkServices,
};
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, OnceCell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_NO_MEMORY, ERR_STATE};

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
    files: OnceCell<FilesServices>,
    library: OnceCell<LibServices>,
    lookup: OnceCell<LookupServices>,
    hyphen: OnceCell<HyphenServices>,
    images: super::image_service::ImageTable,
    ui: OnceCell<UiServices>,
    device: OnceCell<super::device_service::DeviceServices>,
    net: OnceCell<NetServices>,
    work: OnceCell<WorkServices>,
    secrets: OnceCell<SecretsServices>,
    /// The storage paths host writers hold (`core.net` saves), which
    /// `core.files` answers `ERR_BUSY` for. One per device.
    in_use: OnceCell<Rc<super::paths_in_use::PathsInUse>>,
    /// The `core.format` calls under way (root 3 of the file service).
    #[allow(
        dead_code,
        reason = "entered by the DOC adapter, read by the file service"
    )]
    format_scope: super::format_scope::FormatScope,
    /// Owners whose retirement met `ERR_BUSY` where nobody can retry (a
    /// `Drop`): the main loop retries them ([`Self::retry_retirements`]).
    /// Owner ids are never 0. The vector's room is reserved when an owner is
    /// admitted ([`Self::admit_owner`]): its capacity is never below the owners
    /// alive plus the owners queued, so a drop never allocates and never finds
    /// the queue full.
    retiring: RefCell<Vec<u64>>,
    /// Owners alive (admitted, not yet dropped).
    alive: Cell<usize>,
    /// Retirements that could not be queued: a broken invariant, reported by
    /// every retry (`ERR_FULL`) and never cleared.
    lost: Cell<u32>,
}

impl core::fmt::Debug for HostServices {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostServices")
            .field("documents", &self.documents.get().is_some())
            .field("files", &self.files.get().is_some())
            .field("library", &self.library.get().is_some())
            .field("lookup", &self.lookup.get().is_some())
            .field("hyphen", &self.hyphen.get().is_some())
            .field("ui", &self.ui.get().is_some())
            .field("device", &self.device.get().is_some())
            .field("net", &self.net.get().is_some())
            .field("work", &self.work.get().is_some())
            .field("secrets", &self.secrets.get().is_some())
            .finish()
    }
}

impl HostServices {
    pub(crate) const fn new() -> Self {
        Self {
            documents: OnceCell::new(),
            files: OnceCell::new(),
            library: OnceCell::new(),
            lookup: OnceCell::new(),
            hyphen: OnceCell::new(),
            images: super::image_service::ImageTable::new(),
            ui: OnceCell::new(),
            device: OnceCell::new(),
            net: OnceCell::new(),
            work: OnceCell::new(),
            secrets: OnceCell::new(),
            in_use: OnceCell::new(),
            format_scope: super::format_scope::FormatScope::new(),
            retiring: RefCell::new(Vec::new()),
            alive: Cell::new(0),
            lost: Cell::new(0),
        }
    }

    /// A new owner is admitted: room for its retirement is reserved now, so
    /// that its drop can queue it without allocating.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (the owner is not admitted).
    pub(crate) fn admit_owner(&self) -> Result<(), i32> {
        let mut queue = self.retiring.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let len = queue.len();
        let need = len + self.alive.get() + 1;
        if queue.capacity() < need {
            queue
                .try_reserve_exact(need - len)
                .map_err(|_| ERR_NO_MEMORY)?;
        }
        self.alive.set(self.alive.get() + 1);
        Ok(())
    }

    /// An owner is gone; `retired` says its retirement ran to the end. If not,
    /// it is queued for [`Self::retry_retirements`] (in the room reserved at
    /// admission).
    pub(crate) fn owner_gone(&self, owner: u64, retired: bool) {
        self.alive.set(self.alive.get().saturating_sub(1));
        if retired {
            return;
        }
        let queued = self.retiring.try_borrow_mut().is_ok_and(|mut queue| {
            if queue.contains(&owner) {
                return true;
            }
            if queue.len() < queue.capacity() {
                queue.push(owner);
                true
            } else {
                false
            }
        });
        if !queued {
            self.lost.set(self.lost.get().saturating_add(1));
        }
    }

    /// Whether an owner still waits for its retirement.
    pub(crate) fn retirements_pending(&self) -> bool {
        self.lost.get() != 0
            || self
                .retiring
                .try_borrow()
                .map_or(true, |queue| queue.iter().any(|owner| *owner != 0))
    }

    /// The main loop's retry: finish the retirement of every queued owner. An
    /// owner leaves the queue when its retirement ran to the end; one that is
    /// still busy stays. Call again after `Ok(true)` and after `Err(ERR_BUSY)`.
    ///
    /// # Errors
    /// The first error of an owner that stays queued (`ERR_BUSY`);
    /// `ERR_FULL` when a retirement was ever lost.
    pub(crate) fn retry_retirements(&self) -> Result<bool, i32> {
        let mut result = if self.lost.get() == 0 {
            Ok(())
        } else {
            Err(ERR_FULL)
        };
        let mut index = 0;
        loop {
            // No borrow is held across the retirement: it calls into hosts.
            let owner = {
                let queue = self.retiring.try_borrow().map_err(|_| ERR_BUSY)?;
                match queue.get(index) {
                    Some(owner) => *owner,
                    None => break,
                }
            };
            index += 1;
            if owner == 0 {
                continue;
            }
            match self.owner_retired(owner) {
                Ok(()) => {
                    if let Ok(mut queue) = self.retiring.try_borrow_mut()
                        && let Some(slot) = queue.get_mut(index - 1)
                    {
                        *slot = 0;
                    }
                }
                Err(error) => result = result.and(Err(error)),
            }
        }
        if let Ok(mut queue) = self.retiring.try_borrow_mut() {
            queue.retain(|owner| *owner != 0);
        }
        result.map(|()| self.retirements_pending())
    }

    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.document` once. `ERR_STATE` when it is wired already.
    pub(crate) fn wire_documents(&self, host: Rc<dyn DocHost>) -> Result<(), i32> {
        self.documents.set(DocServices(host)).map_err(|_| ERR_STATE)
    }
    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.files` once.
    pub(crate) fn wire_files(&self, host: Rc<dyn FilesHost>) -> Result<(), i32> {
        self.files.set(FilesServices(host)).map_err(|_| ERR_STATE)
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

    /// Wire `text.hyphenate` once.
    pub(crate) fn wire_hyphen(
        &self,
        host: Rc<dyn super::hyphen_service::HyphenHost>,
    ) -> Result<(), i32> {
        self.hyphen
            .set(HyphenServices(Rc::new(
                super::hyphen_service::HyphenState::new(host),
            )))
            .map_err(|_| ERR_STATE)
    }

    /// The device's decoded pictures (`image.info`, `canvas.draw_image`).
    pub(crate) fn images(&self) -> &super::image_service::ImageTable {
        &self.images
    }

    pub(crate) fn hyphen(&self) -> Option<&HyphenServices> {
        self.hyphen.get()
    }

    #[allow(dead_code, reason = "a shell wires its screen stack; tests do today")]
    /// Wire `core.ui` once.
    pub(crate) fn wire_ui(&self, ui: UiServices) -> Result<(), i32> {
        self.ui.set(ui).map_err(|_| ERR_STATE)
    }

    pub(crate) fn ui(&self) -> Option<&UiServices> {
        self.ui.get()
    }

    /// Wire `core.device` once.
    pub(crate) fn wire_device(
        &self,
        host: Rc<dyn super::device_service::DeviceHost>,
    ) -> Result<(), i32> {
        self.device
            .set(super::device_service::DeviceServices(host))
            .map_err(|_| ERR_STATE)
    }

    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.net` once.
    pub(crate) fn wire_net(&self, host: Rc<dyn NetHost>) -> Result<(), i32> {
        self.net.set(NetServices(host)).map_err(|_| ERR_STATE)
    }

    /// The paths in use by host writers (shared with `core.files`' busy check).
    pub(crate) fn in_use(&self) -> &Rc<super::paths_in_use::PathsInUse> {
        self.in_use.get_or_init(Default::default)
    }

    pub(crate) fn net(&self) -> Option<&NetServices> {
        self.net.get()
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

    /// The `core.format` calls under way: the document service enters one
    /// around each call, the file service asks it for root 3.
    #[allow(
        dead_code,
        reason = "entered by the DOC adapter, read by the file service"
    )]
    pub(crate) fn format_scope(&self) -> &super::format_scope::FormatScope {
        &self.format_scope
    }
    pub(crate) fn documents(&self) -> Option<&DocServices> {
        self.documents.get()
    }
    pub(crate) fn files(&self) -> Option<&FilesServices> {
        self.files.get()
    }
    pub(crate) fn library(&self) -> Option<&LibServices> {
        self.library.get()
    }
    pub(crate) fn lookup(&self) -> Option<&LookupServices> {
        self.lookup.get()
    }
    pub(crate) fn device(&self) -> Option<&super::device_service::DeviceServices> {
        self.device.get()
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
        // The plugin's pictures end with it (its image ids are its own).
        result = result.and(self.images.owner_stopped(owner));
        // What the owner made for the screen stack is freed; no plugin is called.
        if let Some(ui) = self.ui.get() {
            result = result.and(ui.store.owner_stopped(owner));
        }
        if let Some(net) = self.net.get() {
            result = result.and(net_service::owner_stopped(&*net.0, owner));
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
