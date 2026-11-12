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
    sock_service::{self, SockHost, SockServices},
    ui_service::UiServices,
    work_service::WorkServices,
};
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, OnceCell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_NO_MEMORY, ERR_STATE, ERR_UNSUPPORTED};

/// The most documents of one maker the host ends in one go (more than the
/// device holds).
const HANDED_ENDS: usize = 64;

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
    /// What a native screen's callbacks stage, for the kernel (`screen_stage`).
    pub(super) screen_stage: super::screen_stage::ScreenStage,
    documents: OnceCell<DocServices>,
    files: OnceCell<FilesServices>,
    library: OnceCell<LibServices>,
    /// Boxed: its table of pending asks is 1 KiB, held only where it is wired.
    lookup: OnceCell<alloc::boxed::Box<LookupServices>>,
    hyphen: OnceCell<HyphenServices>,
    /// The card, for the font files of a plugin's data folder (`font.acquire`
    /// `data:<path>`).
    font_card: OnceCell<Rc<dyn paperpaw_platform::Storage>>,
    images: super::image_service::ImageTable,
    ui: OnceCell<alloc::boxed::Box<UiServices>>,
    device: OnceCell<super::device_service::DeviceServices>,
    /// The firmware install machine, which `core.device` ops 3, 6 and 16 read
    /// and the kernel's pump drives. One per device.
    firmware: OnceCell<Rc<RefCell<super::firmware_install::FirmwareInstall>>>,
    /// The firmware request table (op 13 reads the state of a request and the
    /// outcome kept for it). One per device.
    firmware_requests: OnceCell<Rc<RefCell<super::firmware_driver::Driver>>>,
    /// The kernel's table of Wi-Fi requests, which a callback's stage
    /// reserves `join` and `host` in (`commands_dev.rs`). One per device.
    radio_requests: OnceCell<Rc<RefCell<super::radio_requests::RadioRequests>>>,
    net: OnceCell<NetServices>,
    sockets: OnceCell<SockServices>,
    work: OnceCell<WorkServices>,
    secrets: OnceCell<SecretsServices>,
    /// The library's move job and its card side, made when `core.files` is
    /// wired (`lib_moves_service`).
    pub(super) library_moves: OnceCell<Rc<super::lib_moves_service::LibraryMoves>>,
    /// What the host holds in use, for `core.files` to answer busy about.
    busy: OnceCell<Rc<super::storage_files::BusySources>>,
    /// The ids of the plugins linked into the firmware (root 2 of the file
    /// service has no install folder for them).
    linked: OnceCell<Rc<super::linked_plugins::LinkedPlugins>>,
    /// The folders of the running data plugins (root 1 shows them read only).
    data_plugins: OnceCell<Rc<super::data_plugin_folders::DataPluginFolders>>,
    /// The device's facts the library's transactions read (memory class,
    /// clock), told by the wirings in any order.
    edit_policy: OnceCell<Rc<super::lib_edits::EditPolicy>>,
    /// The storage paths host writers hold (`core.net` saves), which
    /// `core.files` answers `ERR_BUSY` for. One per device.
    in_use: OnceCell<Rc<super::paths_in_use::PathsInUse>>,
    /// What the user decided about the network (`net_policy`), kept by the
    /// kernel and read by `core.net` and `core.sockets`.
    policy: OnceCell<Rc<super::net_policy::NetPolicy>>,
    /// The device's one network pool, shared by `core.net` and
    /// `core.sockets`.
    pool: OnceCell<Rc<super::net_pool::NetPool>>,
    /// The enabled owners' plugin ids and subscriptions, kept by the owner
    /// registry for the `core.net` adapter.
    owners: OnceCell<super::shared::Shared<super::owner_directory::OwnerDirectory>>,
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
            .field("firmware", &self.firmware.get().is_some())
            .field("net", &self.net.get().is_some())
            .field("sockets", &self.sockets.get().is_some())
            .field("work", &self.work.get().is_some())
            .field("secrets", &self.secrets.get().is_some())
            .finish()
    }
}

impl HostServices {
    pub(crate) const fn new() -> Self {
        Self {
            screen_stage: super::screen_stage::ScreenStage::new(),
            documents: OnceCell::new(),
            files: OnceCell::new(),
            library: OnceCell::new(),
            lookup: OnceCell::new(),
            hyphen: OnceCell::new(),
            font_card: OnceCell::new(),
            images: super::image_service::ImageTable::new(),
            ui: OnceCell::new(),
            device: OnceCell::new(),
            firmware: OnceCell::new(),
            firmware_requests: OnceCell::new(),
            radio_requests: OnceCell::new(),
            net: OnceCell::new(),
            sockets: OnceCell::new(),
            work: OnceCell::new(),
            secrets: OnceCell::new(),
            library_moves: OnceCell::new(),
            busy: OnceCell::new(),
            linked: OnceCell::new(),
            data_plugins: OnceCell::new(),
            edit_policy: OnceCell::new(),
            in_use: OnceCell::new(),
            policy: OnceCell::new(),
            pool: OnceCell::new(),
            owners: OnceCell::new(),
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

    /// Owners alive now (tests).
    #[cfg(test)]
    pub(crate) fn alive_owners(&self) -> usize {
        self.alive.get()
    }

    /// A retirement that could not be queued (tests: the broken invariant).
    #[cfg(test)]
    pub(crate) fn lose_a_retirement(&self) {
        self.lost.set(self.lost.get().saturating_add(1));
    }

    /// Whether `owner` still waits for its retirement (queued).
    ///
    /// # Errors
    /// `ERR_BUSY` while the queue is in use; `ERR_STATE` once a retirement
    /// was lost (which owner's is not known, so none is sure to be done).
    pub(crate) fn retiring(&self, owner: u64) -> Result<bool, i32> {
        if self.lost.get() != 0 {
            return Err(ERR_STATE);
        }
        let queue = self.retiring.try_borrow().map_err(|_| ERR_BUSY)?;
        Ok(queue.contains(&owner))
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
    pub(crate) fn wire_lookup(
        &self,
        host: Rc<dyn LookupHost>,
        log: super::device_log::DeviceLog,
    ) -> Result<(), i32> {
        let made = super::shared::try_box(LookupServices::new(host, log)).ok_or(ERR_NO_MEMORY)?;
        self.lookup.set(made).map_err(|_| ERR_STATE)
    }

    /// Wire the card the data fonts are opened on, once.
    pub(crate) fn wire_font_card(
        &self,
        card: Rc<dyn paperpaw_platform::Storage>,
    ) -> Result<(), i32> {
        self.font_card.set(card).map_err(|_| ERR_STATE)
    }

    /// The card of the data fonts, if wired.
    pub(crate) fn font_card(&self) -> Option<&Rc<dyn paperpaw_platform::Storage>> {
        self.font_card.get()
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
        let made = super::shared::try_box(ui).ok_or(ERR_NO_MEMORY)?;
        self.ui.set(made).map_err(|_| ERR_STATE)
    }

    pub(crate) fn ui(&self) -> Option<&UiServices> {
        self.ui.get().map(|ui| &**ui)
    }

    /// The file the hand-over of reader `instance` names, for `doc_open`
    /// root 5 (see [`super::ui_store::UiStore::handed_file`]); the instance is
    /// the caller's (`Frame::calling_instance`). `ERR_UNSUPPORTED` while no
    /// `core.ui` store is wired.
    pub(crate) fn handed_file(
        &self,
        instance: u32,
        visit: &mut dyn FnMut(super::ui_store::Handed<'_>),
    ) -> Result<(), i32> {
        self.ui()
            .ok_or(paperpaw_native::host_glue::ERR_UNSUPPORTED)?
            .store
            .handed_file(instance, visit)
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

    /// Wire the firmware request table once (op 13).
    pub(crate) fn wire_firmware_requests(
        &self,
        driver: Rc<RefCell<super::firmware_driver::Driver>>,
    ) -> Result<(), i32> {
        self.firmware_requests.set(driver).map_err(|_| ERR_STATE)
    }

    /// The firmware request table, where wired.
    pub(crate) fn firmware_requests(&self) -> Option<&Rc<RefCell<super::firmware_driver::Driver>>> {
        self.firmware_requests.get()
    }

    /// Wire the kernel's Wi-Fi request table; the first wiring stays (the
    /// kernel has one table).
    pub(crate) fn wire_radio_requests(
        &self,
        table: &Rc<RefCell<super::radio_requests::RadioRequests>>,
    ) {
        let _ = self.radio_requests.set(Rc::clone(table));
    }

    /// The kernel's Wi-Fi request table, where wired.
    pub(crate) fn radio_requests(
        &self,
    ) -> Option<&Rc<RefCell<super::radio_requests::RadioRequests>>> {
        self.radio_requests.get()
    }

    /// Wire the firmware install machine once.
    #[allow(
        dead_code,
        reason = "the kernel's pump wires it (slice B2); tests do today"
    )]
    pub(crate) fn wire_firmware_install(
        &self,
        machine: Rc<RefCell<super::firmware_install::FirmwareInstall>>,
    ) -> Result<(), i32> {
        self.firmware.set(machine).map_err(|_| ERR_STATE)
    }

    #[allow(dead_code, reason = "a shell wires its host view; tests do today")]
    /// Wire `core.net` once.
    pub(crate) fn wire_net(&self, host: Rc<dyn NetHost>) -> Result<(), i32> {
        self.net.set(NetServices(host)).map_err(|_| ERR_STATE)
    }

    /// The device's one network pool, made by the first wiring that names a
    /// size and a number of TLS sessions. A device has one pool: a later
    /// wiring that names another shape is refused, not ignored.
    ///
    /// # Errors
    /// `ERR_INVALID` when the pool exists with another size or number of
    /// sessions.
    pub(crate) fn net_pool(
        &self,
        capacity: u32,
        tls_sessions: u8,
    ) -> Result<&Rc<super::net_pool::NetPool>, i32> {
        let pool = self.pool.get_or_init(|| {
            Rc::new(super::net_pool::NetPool::with_tls_sessions(
                capacity,
                tls_sessions,
            ))
        });
        if pool.capacity() == capacity
            && pool.tls_sessions() == super::net_pool::NetPool::clamp_sessions(tls_sessions)
        {
            Ok(pool)
        } else {
            Err(paperpaw_native::host_glue::ERR_INVALID)
        }
    }

    /// The device's network policy, made on first use.
    pub(crate) fn policy(&self) -> &Rc<super::net_policy::NetPolicy> {
        self.policy
            .get_or_init(|| Rc::new(super::net_policy::NetPolicy::new()))
    }

    /// What the wirings tell the library's transactions, made on first use.
    pub(crate) fn edit_policy(&self) -> &Rc<super::lib_edits::EditPolicy> {
        self.edit_policy.get_or_init(Default::default)
    }

    /// Gives back the room kept while the plugins were hosted one by one.
    pub(crate) fn shrink_to_fit(&self) {
        if let Some(linked) = self.linked.get() {
            linked.shrink_to_fit();
        }
    }

    /// The linked plugin ids, made on first use.
    pub(crate) fn linked(&self) -> &Rc<super::linked_plugins::LinkedPlugins> {
        self.linked.get_or_init(Default::default)
    }

    /// The running data plugins' folders, made on first use.
    pub(crate) fn data_plugins(&self) -> &Rc<super::data_plugin_folders::DataPluginFolders> {
        self.data_plugins.get_or_init(Default::default)
    }

    /// The device's network pool, if a service has made it.
    pub(crate) fn pool_made(&self) -> Option<&Rc<super::net_pool::NetPool>> {
        self.pool.get()
    }

    /// Tells the shell that the combined demand for the network changed
    /// (through the service that is wired).
    pub(crate) fn tell_demand(&self, wanted: bool) {
        if let Some(net) = self.net() {
            net.0.network_demand(wanted);
        } else if let Some(sockets) = self.sockets() {
            sockets.0.network_demand(wanted);
        }
    }

    /// Asks the work scheduler again for the closes of runs' resources it
    /// could not take (see `OwnerDirectory::retry_closes`).
    pub(crate) fn retry_run_closes(&self, now_ms: u64) {
        if let Some(owners) = self.owners.get()
            && owners.closes_wait()
        {
            owners.retry_closes(now_ms);
        }
    }

    /// The paths in use by host writers (shared with `core.files`' busy check).
    pub(crate) fn in_use(&self) -> &Rc<super::paths_in_use::PathsInUse> {
        self.in_use.get_or_init(Default::default)
    }

    /// Makes the in-use table (what `core.net` and `core.sockets` hold) a
    /// source of the busy answers of `core.files`, once for the device.
    ///
    /// # Errors
    /// `ERR_BUSY` when the list of sources is in use; `ERR_NO_MEMORY`.
    pub(crate) fn register_in_use_source(&self) -> Result<(), i32> {
        let sources = self.busy_sources();
        if sources.has(super::storage_files::BusyKind::Network) {
            return Ok(());
        }
        sources.register(
            super::storage_files::BusyKind::Network,
            self.in_use().clone(),
        )
    }

    /// The document service reports the books it has open
    /// ([`reader_book_opened`](Self::reader_book_opened) and
    /// [`reader_book_closed`](Self::reader_book_closed)): the in-use table is
    /// the reader-book source of `core.files`. Called once, by the document
    /// service's wiring, and only where it makes those calls: without them
    /// "no book is open" would be a wrong answer, so `core.files` stays
    /// unwired until this has been called.
    ///
    /// # Errors
    /// `ERR_STATE` when it was called already; `ERR_BUSY` when the list of
    /// sources is in use; `ERR_NO_MEMORY`.
    #[allow(
        dead_code,
        reason = "the document service's wiring calls it (Host Engineer, train 6)"
    )]
    pub(crate) fn register_reader_book_reporting(&self) -> Result<(), i32> {
        let sources = self.busy_sources();
        if sources.has(super::storage_files::BusyKind::ReaderBook) {
            return Err(ERR_STATE);
        }
        sources.register(
            super::storage_files::BusyKind::ReaderBook,
            self.in_use().clone(),
        )
    }

    /// The library's move job is a busy source of `core.files`: called by the
    /// library host once, with the `Moves` and the `MoveHost` it runs the job
    /// with.
    ///
    /// # Errors
    /// `ERR_BUSY` when the list of sources is in use; `ERR_NO_MEMORY`.
    pub(crate) fn register_library_moves(
        &self,
        moves: Rc<super::lib_service::moves::Moves>,
        host: Rc<dyn super::lib_service::moves::MoveHost>,
    ) -> Result<(), i32> {
        self.busy_sources().register(
            super::storage_files::BusyKind::LibraryMoves,
            Rc::new(super::busy_adapters::MovesBusy::new(moves, host)),
        )
    }

    /// A document of the book at `path` was opened through the document
    /// service: plugins' writes, moves and removals of the book (and of a
    /// folder holding it) answer `ERR_BUSY` until the document ends
    /// ([`reader_book_closed`](Self::reader_book_closed)); the host's own
    /// reads of it are not held back. Counted per path: several documents may
    /// hold one book. The document service calls it once
    /// when a document opens and refuses the open on an error, so a book is
    /// never left unguarded.
    ///
    /// # Errors
    /// `ERR_FULL` when the table has no slot for it (the open is refused
    /// with it); `ERR_BUSY` when the table is borrowed.
    #[allow(
        dead_code,
        reason = "the document service calls it (Host Engineer, train 6)"
    )]
    pub(crate) fn reader_book_opened(&self, path: &str) -> Result<(), i32> {
        use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_MEMORY};
        self.in_use()
            .claim(super::busy_adapters::READER_OWNER, path)
            .map_err(|status| {
                if status == ERR_NO_MEMORY {
                    ERR_FULL
                } else {
                    status
                }
            })
    }

    /// A document of the book at `path` ended (an explicit close, its opener
    /// or format stopping, the card going, the book moved or removed, its
    /// place lost to another book): one hold on the path is given back. The
    /// document service calls it once for each document that ended, with the
    /// exact path it opened with. Whether there was a hold to give back: a
    /// `false` is an unbalanced close, for the caller to surface (log).
    #[allow(
        dead_code,
        reason = "the document service calls it (Host Engineer, train 6)"
    )]
    pub(crate) fn reader_book_closed(&self, path: &str) -> bool {
        self.in_use()
            .release(super::busy_adapters::READER_OWNER, path)
    }

    /// The owner directory, made on first use.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn owners(
        &self,
    ) -> Result<&super::shared::Shared<super::owner_directory::OwnerDirectory>, i32> {
        if let Some(directory) = self.owners.get() {
            return Ok(directory);
        }
        let made = super::shared::Shared::try_new(super::owner_directory::OwnerDirectory::new())
            .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)?;
        let _ = self.owners.set(made);
        self.owners.get().ok_or(ERR_STATE)
    }

    pub(crate) fn net(&self) -> Option<&NetServices> {
        self.net.get()
    }

    /// The wired services that raise events to plugins, for the kernel's
    /// event pump: `core.net`, `core.sockets`, then `core.document`.
    pub(crate) fn event_services(&self) -> [Option<&dyn super::service_events::ServiceEvents>; 3] {
        [
            self.net
                .get()
                .map(|s| s as &dyn super::service_events::ServiceEvents),
            self.sockets
                .get()
                .map(|s| s as &dyn super::service_events::ServiceEvents),
            self.documents
                .get()
                .map(|s| s as &dyn super::service_events::ServiceEvents),
        ]
    }

    #[allow(dead_code, reason = "a shell wires its sockets; tests do today")]
    /// Wire `core.sockets` once.
    pub(crate) fn wire_sockets(&self, host: Rc<dyn SockHost>) -> Result<(), i32> {
        self.sockets.set(SockServices(host)).map_err(|_| ERR_STATE)
    }

    pub(crate) fn sockets(&self) -> Option<&SockServices> {
        self.sockets.get()
    }

    /// Wire `core.work` once.
    pub(crate) fn wire_work(&self, work: WorkServices) -> Result<(), i32> {
        let owners = self.owners()?;
        let sched = Rc::clone(&work.sched);
        self.work.set(work).map_err(|_| ERR_STATE)?;
        owners.attach_sched(&sched);
        Ok(())
    }

    pub(crate) fn work(&self) -> Option<&WorkServices> {
        self.work.get()
    }

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
    /// The host ends one document on its own (`doc_service::end_by_host`) and
    /// its image ids end at once, here where the image table is at hand: a
    /// stale id must not draw what a later cache holds under the same name.
    /// Only a borrowed table defers them (to the next call).
    ///
    /// # Errors
    /// `ERR_UNSUPPORTED` without a document service; `end_by_host`'s.
    #[allow(dead_code, reason = "the card and move hooks call it; tests do today")]
    pub(crate) fn end_document_by_host(&self, id: u32, reason: u32) -> Result<(), i32> {
        let documents = self.documents().ok_or(ERR_UNSUPPORTED)?;
        let result = doc_service::end_by_host(&*documents.0, id, reason);
        documents.0.flush_images(&self.images);
        result
    }
    /// The sources of paths in use, made on first use.
    pub(crate) fn busy_sources(&self) -> &Rc<super::storage_files::BusySources> {
        self.busy.get_or_init(Default::default)
    }
    pub(crate) fn files(&self) -> Option<&FilesServices> {
        self.files.get()
    }
    pub(crate) fn library(&self) -> Option<&LibServices> {
        self.library.get()
    }
    pub(crate) fn lookup(&self) -> Option<&LookupServices> {
        self.lookup.get().map(|lookup| &**lookup)
    }
    pub(crate) fn device(&self) -> Option<&super::device_service::DeviceServices> {
        self.device.get()
    }
    pub(crate) fn firmware_install(
        &self,
    ) -> Option<&Rc<RefCell<super::firmware_install::FirmwareInstall>>> {
        self.firmware.get()
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
            // A book of this plugin's folders (root 5) cannot stay open under
            // a reader once the folder may go (its update or removal): the
            // documents end now (reason 3).
            for _ in 0..HANDED_ENDS {
                let Some(id) = host.document_of_maker(owner) else {
                    break;
                };
                if let Err(status) = doc_service::end_by_host(host, id, 3) {
                    result = result.and(Err(status));
                    break;
                }
            }
            // (The pictures of a format are not mapped yet in this build: no
            // image id needs ending here; the picture slice, #991, ends them
            // through `end_document_by_host`.)
        }
        if let Some(library) = self.library.get() {
            result = result.and(lib_service::owner_stopped(&*library.0, owner));
        }
        // The plugin's pictures end with it (its image ids are its own).
        result = result.and(self.images.owner_stopped(owner));
        // What the owner made for the screen stack is freed; no plugin is called.
        if let Some(ui) = self.ui.get() {
            result = result.and(ui.store.owner_stopped(owner));
            result = result.and(ui.plugin_stopped(owner));
        }
        if let Some(net) = self.net.get() {
            result = result.and(net_service::owner_stopped(&*net.0, owner));
        }
        if let Some(sockets) = self.sockets.get() {
            result = result.and(sock_service::owner_stopped(&*sockets.0, owner));
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
