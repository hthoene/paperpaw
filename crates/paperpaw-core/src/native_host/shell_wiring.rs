//! The shells' one way to give the host the views it serves services from.
//!
//! `HostServices` and its `wire_*` methods stay inside the crate; a shell (the X4, the desktop emulator,
//! Android) describes what it has in a [`HostWiring`] and hands it to
//! [`HostResources::wire`] once, where the device's table is made. Every
//! family asked for is checked unwired before the first is set, so a refusal
//! leaves nothing half wired.
//!
//! Which families a shell can name grows with the adapters: `text.hyphenate`
//! (the card's tables), `core.device` (a host that already is public),
//! `core.ui` (the store; the stack's view comes from each call's frame) and
//! `core.net` (the card and the shell's network), `core.document` and
//! `core.library` (its reads) today; the other families'
//! hosts are core's adapters over kernel objects and join as their adapters
//! land, one method each.
use super::{
    device_service::DeviceHost,
    hyphen_card::CardTables,
    lib_host::LibraryHost,
    resources::HostResources,
    secrets_service::SecretsServices,
    shared::Shared,
    storage_files::StorageFiles,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
    work_service::WorkServices,
};

#[path = "shell_wiring/net.rs"]
mod net;
#[path = "shell_wiring/secrets.rs"]
mod secrets;
#[path = "shell_wiring/sockets.rs"]
mod sockets;
#[path = "shell_wiring/work.rs"]
mod work;
pub use super::lib_reads::CatalogEnv;
pub use super::plugin_heap::{PLUGIN_HEAP_LARGE, PLUGIN_HEAP_SMALL, default_plugin_heap};
pub use super::sock_service::{Connect, PeerInfo, SockTls, SockTransport, TlsOpen, TlsPin};
use alloc::{rc::Rc, vec::Vec};
pub use net::{
    HttpDone, HttpHeaders, HttpJob, HttpReason, NetInputs, NetLimits, NetReports, ServedDecision,
    ServedReply, ServerReason, ShellHttp, ShellNetwork,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_platform::Storage;
pub use secrets::{Rng, SEALED_MAX, Sealer, SecretsInputs, SecretsLifecycle};
pub use sockets::{SockFailure, SockInputs, SockLimits, SockReports, TlsFailure, UntrustedWhy};
pub(crate) use work::make_work;
pub use work::{WorkDevice, WorkInputs};

/// What a shell wires into the device's host views. Nothing is wired for a
/// family the shell does not name.
#[derive(Default)]
#[non_exhaustive]
pub struct HostWiring {
    hyphenation: Option<Rc<dyn Storage>>,
    files: Option<Rc<dyn Storage>>,
    device: Option<Rc<dyn DeviceHost>>,
    net: Option<NetInputs>,
    sockets: Option<SockInputs>,
    /// `core.ui`: `Some(x4)`.
    ui: Option<bool>,
    /// `core.document`: the memory class and the screen's long side.
    documents: Option<(paperpaw_platform::MemoryClass, u32)>,
    document_colour: bool,
    /// `core.library`: the catalog's card and the shell's facts.
    library: Option<(Rc<dyn Storage>, Rc<dyn CatalogEnv>)>,
    /// `core.lookup`.
    lookup: bool,
    secrets: Option<SecretsInputs>,
    work: Option<WorkInputs>,
    /// The per-plugin heap budget the shell named.
    plugin_heap: Option<usize>,
}

impl core::fmt::Debug for HostWiring {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostWiring")
            .field("hyphenation", &self.hyphenation.is_some())
            .field("files", &self.files.is_some())
            .field("device", &self.device.is_some())
            .field("net", &self.net.is_some())
            .field("sockets", &self.sockets.is_some())
            .field("ui", &self.ui.is_some())
            .field("documents", &self.documents.is_some())
            .field("library", &self.library.is_some())
            .field("lookup", &self.lookup)
            .field("secrets", &self.secrets.is_some())
            .field("work", &self.work.is_some())
            .field("plugin_heap", &self.plugin_heap)
            .finish()
    }
}

impl HostWiring {
    /// The default budget of the classes this wiring names (the smaller of
    /// two; Small where it names none).
    pub(crate) fn class_plugin_heap(&self) -> usize {
        let small = self.ui == Some(true)
            || self
                .documents
                .is_some_and(|(memory, _)| super::ui_store::is_x4(memory));
        let named = self.ui.is_some() || self.documents.is_some();
        if named && !small {
            super::plugin_heap::PLUGIN_HEAP_LARGE
        } else {
            super::plugin_heap::PLUGIN_HEAP_SMALL
        }
    }

    /// Nothing named yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `text.hyphenate`: the data plugins' hyphenation tables are files on
    /// this storage (the card). The shell then declares which languages the
    /// installed plugins offer ([`HyphenTables::declare`]).
    #[must_use]
    pub fn hyphenation(mut self, storage: Rc<dyn Storage>) -> Self {
        self.hyphenation = Some(storage);
        self
    }

    /// `core.files`: the plugins' file service over this storage (the card).
    /// The shell gives only the storage; core composes the rest: the host's
    /// own folders by identity (an 8.3 alias of `.paperpaw` is refused; the
    /// check over-refuses until the storage can answer "same entry"), what is
    /// in use (downloads, staged installs, served bodies, the library's move
    /// job: `ERR_BUSY`) and the move itself. [`HostResources::wire`] refuses
    /// it (`ERR_STATE`, nothing wired) until the host has a source for each of
    /// the library's move job, the network's files and the reader's open book:
    /// nothing busy would be a wrong answer for state the host does hold. The
    /// sources are core's adapters and register as they land; a shell treats
    /// the refusal as "no `core.files`" and carries on.
    #[must_use]
    pub fn files(mut self, storage: Rc<dyn Storage>) -> Self {
        self.files = Some(storage);
        self
    }

    /// `core.device`: the shell's view of its power, radio and firmware.
    #[must_use]
    pub fn device(mut self, host: Rc<dyn DeviceHost>) -> Self {
        self.device = Some(host);
        self
    }

    /// `core.net`: HTTP requests and serving, over the card and the shell's
    /// network ([`NetInputs`]). The transport's reports come back through
    /// [`WiredHosts::net`].
    ///
    /// Wire before `Kernel::start`. The kernel fills the user's network policy
    /// (the power mode and each plugin's network setting) at its first pass and
    /// at every change after; until then the policy is closed, and a plugin that
    /// asks first is answered `network_off`. Wired after the kernel has started,
    /// the services answer `network_off` to a plugin's first requests until the
    /// kernel's next event or call end.
    #[must_use]
    pub fn net(mut self, inputs: NetInputs) -> Self {
        self.net = Some(inputs);
        self
    }

    /// `core.sockets`: TCP sockets (and TLS, if the inputs have it) and UDP
    /// broadcast, over the card and the shell's network ([`SockInputs`]). The
    /// transport's reports come back through [`WiredHosts::sockets`]. The
    /// network pool is the one `core.net` uses: name the same size in both.
    ///
    /// Wire before `Kernel::start`. The kernel fills the user's network policy
    /// (the power mode and each plugin's network setting) at its first pass and
    /// at every change after; until then the policy is closed, and a plugin that
    /// asks first is answered `network_off`. Wired after the kernel has started,
    /// the services answer `network_off` to a plugin's first requests until the
    /// kernel's next event or call end.
    #[must_use]
    pub fn sockets(mut self, inputs: SockInputs) -> Self {
        self.sockets = Some(inputs);
        self
    }

    /// `core.ui`: the screen-stack service (questions, hand-overs, texts). The
    /// host makes its store, sized for the device's memory class (`Small`
    /// gets the small limits of a 256 KiB heap; the same test as the
    /// admission rules use, so the two cannot disagree); nothing else is passed
    /// in, and nothing is allocated until a plugin uses it.
    #[must_use]
    pub fn ui(mut self, memory: paperpaw_platform::MemoryClass) -> Self {
        self.ui = Some(super::ui_store::is_x4(memory));
        self
    }

    /// `core.document`: the books plugins open and the formats that read
    /// them (`docs/abi/document.md`). The host makes its table (2 documents
    /// on the device for a `Small` memory class, 8 elsewhere) and reads the
    /// formats and reading apps from the plugins' declarations; the shell
    /// names only the screen's long side in pixels (the bound of a cover).
    /// Wiring it makes the host the source of "the reader's open book" that
    /// `core.files` needs, so `files` may be named in the same wiring.
    #[must_use]
    pub fn documents(
        mut self,
        memory: paperpaw_platform::MemoryClass,
        screen_long_side: u32,
    ) -> Self {
        self.documents = Some((memory, screen_long_side));
        self
    }

    /// The most heap one plugin may use, in bytes (`abi/heap-word.md`
    /// section 4): a ceiling, not a reservation (a full real heap still gives
    /// null), and the headers and the padding an alignment needs count toward
    /// it. 0 makes every allocation null.
    ///
    /// Without it the budget is the default of the memory class the wiring
    /// names ([`default_plugin_heap`]): [`PLUGIN_HEAP_SMALL`] on the X4,
    /// [`PLUGIN_HEAP_LARGE`] elsewhere; where `ui` and `documents` name
    /// different classes the smaller applies, and where neither names one,
    /// the small one. The X4's value can change after the device measurement.
    ///
    /// The limit is read at each allocation: one set below what a plugin uses
    /// reclaims nothing, and that plugin's later allocations are null until
    /// its use drops below the limit.
    #[must_use]
    pub fn plugin_heap(mut self, bytes: usize) -> Self {
        self.plugin_heap = Some(bytes);
        self
    }

    /// The screen keeps colour: a book's pictures that are in colour say so
    /// (`doc_image.flags` bit 0). Without it, never. It must match the
    /// shell's real panel: a shell whose screen is grey says `false`.
    #[must_use]
    pub fn document_colour(mut self, colour: bool) -> Self {
        self.document_colour = colour;
        self
    }

    /// `core.library`: the reads of the library's catalog on this storage (the
    /// card), with what the card cannot tell: whether the walk is making the
    /// catalog, today's date, the format of a file ([`CatalogEnv`]). Only the
    /// reads are served: every write op answers `ERR_UNSUPPORTED` (the
    /// production write half is todo 307), and a name below the card's top
    /// is one of the host's folders by spelling or, for an 8.3 alias (a name
    /// with a `~` where one of those names also exists), as `core.files`
    /// decides it. Where `core.files` is wired too (in the same wiring or an
    /// earlier one: name `files` first, or together), a book its library move
    /// job carries is in use: `provide` answers `ERR_BUSY` for it. Wired
    /// before `files`, the library has no such check.
    #[must_use]
    pub fn library(mut self, storage: Rc<dyn Storage>, env: Rc<dyn CatalogEnv>) -> Self {
        self.library = Some((storage, env));
        self
    }

    /// `core.lookup`: the dictionary, wikipedia and translate providers the
    /// plugins declared (`docs/abi/document.md` 7). The host reads them from
    /// the declarations, calls them in load order and keeps nothing; the shell
    /// names nothing.
    #[must_use]
    pub fn lookup(mut self) -> Self {
        self.lookup = true;
        self
    }

    /// `core.secrets`: the plugins' secrets file on this storage, kept by the
    /// shell's [`Sealer`] ([`SecretsInputs`]). The calls the shell makes
    /// itself (upgrade at start, forgetting a deleted plugin's secrets) come
    /// back through [`WiredHosts::secrets`].
    #[must_use]
    pub fn secrets(mut self, inputs: SecretsInputs) -> Self {
        self.secrets = Some(inputs);
        self
    }

    /// `core.work`: declared background work, over the device's clock and
    /// radio ([`WorkInputs`]). Core owns the scheduler and composes the rest
    /// (the user's power level, the plugins' network settings, who
    /// subscribed); the kernel runs the passes.
    ///
    /// Wire before `Kernel::start`: the user's network policy is closed until
    /// the kernel has filled it at its first pass, and a network work declared
    /// before is refused as `network_off`.
    #[must_use]
    pub fn work(mut self, inputs: WorkInputs) -> Self {
        self.work = Some(inputs);
        self
    }
}

/// The declared hyphenation languages and the card's changes, for the shell to
/// keep current: plugins installed or removed, a card inserted or written.
#[derive(Clone)]
pub struct HyphenTables(Rc<CardTables>);

impl core::fmt::Debug for HyphenTables {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HyphenTables")
    }
}

impl HyphenTables {
    pub(crate) fn over(tables: Rc<CardTables>) -> Self {
        Self(tables)
    }

    #[cfg(test)]
    pub(crate) fn card(&self) -> &CardTables {
        &self.0
    }

    /// Replaces the declared tables: `(language tag, card path, plugin id)`
    /// for each installed plugin's `resource.hyphenation` entry, in any order
    /// (the greatest plugin id wins a tag). All or nothing: on an error the
    /// old ones stay.
    ///
    /// # Errors
    /// `ERR_INVALID` (an empty or overlong tag or path), `ERR_NO_MEMORY`.
    pub fn declare<'a>(
        &self,
        declared: impl Iterator<Item = (&'a str, &'a str, u64)>,
    ) -> Result<(), i32> {
        self.0.replace(declared)
    }

    /// As [`declare`](Self::declare), with the plugin named by its id: the
    /// plugin whose id wins ([`paperpaw_sdk::plugin_wins`], D4) gets a tag
    /// several declare, as the kernel's own tables decide it. The
    /// iterator is walked twice.
    ///
    /// # Errors
    /// As [`declare`](Self::declare).
    pub fn declare_plugins<'a>(
        &self,
        declared: impl Iterator<Item = (&'a str, &'a str, &'a str)> + Clone,
    ) -> Result<(), i32> {
        // The SDK's own rule (D4), so the host and the SDK cannot diverge.
        let order = |a: &str, b: &str| {
            if paperpaw_sdk::plugin_wins(a, b) {
                core::cmp::Ordering::Greater
            } else if paperpaw_sdk::plugin_wins(b, a) {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Equal
            }
        };
        let mut plugins: Vec<&str> = Vec::new();
        for (_, _, plugin) in declared.clone() {
            plugins.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            plugins.push(plugin);
        }
        plugins.sort_unstable_by(|a, b| order(a, b));
        plugins.dedup_by(|a, b| order(a, b).is_eq());
        let rank = |plugin: &str| {
            let at = plugins
                .binary_search_by(|probe| order(probe, plugin))
                .unwrap_or(0);
            at as u64 + 1
        };
        self.0
            .replace(declared.map(|(tag, path, plugin)| (tag, path, rank(plugin))))
    }

    /// The card was inserted, removed or written: open tables are checked
    /// again before they are used.
    pub fn card_changed(&self) {
        self.0.card_changed();
    }
}

/// The handles of what [`HostResources::wire`] wired, for the shell to keep.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct WiredHosts {
    hyphen: Option<HyphenTables>,
    net: Option<NetReports>,
    sockets: Option<SockReports>,
    secrets: Option<SecretsLifecycle>,
    ui_skipped: bool,
}

impl WiredHosts {
    /// The hyphenation tables, if `text.hyphenate` was wired.
    #[must_use]
    pub fn hyphen(&self) -> Option<&HyphenTables> {
        self.hyphen.as_ref()
    }

    /// The transport's reports, if `core.net` was wired.
    #[must_use]
    pub fn net(&self) -> Option<&NetReports> {
        self.net.as_ref()
    }

    /// The transport's reports, if `core.sockets` was wired.
    #[must_use]
    pub fn sockets(&self) -> Option<&SockReports> {
        self.sockets.as_ref()
    }

    /// What the shell calls outside a plugin's call, if `core.secrets` was
    /// wired.
    #[must_use]
    pub fn secrets(&self) -> Option<&SecretsLifecycle> {
        self.secrets.as_ref()
    }

    /// Takes what the shell calls outside a plugin's call, if `core.secrets`
    /// was wired, for a shell that keeps it past the wiring (the X4 seals
    /// interface 6's values once its random numbers are strong).
    pub fn take_secrets(&mut self) -> Option<SecretsLifecycle> {
        self.secrets.take()
    }

    /// Whether `core.ui` was asked for and not wired because its store could
    /// not be allocated (the other families are wired): the shell says so.
    #[must_use]
    pub fn ui_skipped(&self) -> bool {
        self.ui_skipped
    }
}

impl HostResources {
    /// Wires the families `wiring` names, once for the device.
    ///
    /// # Errors
    /// `ERR_STATE` when one of them is wired already, `ERR_INVALID` when
    /// `core.net` and `core.sockets` name different network pools (nothing is
    /// wired then).
    #[allow(
        clippy::too_many_lines,
        reason = "one wiring step per service, in order; each is a few lines"
    )]
    pub fn wire(&self, wiring: HostWiring) -> Result<WiredHosts, i32> {
        let hosts = self.hosts();
        // What `core.net` and `core.sockets` hold in use is a busy source of
        // the device from the start (the table is the host's own, whether
        // or not either service is wired), so a call that names the network
        // and `core.files` together is judged on the other two sources only.
        hosts.register_in_use_source()?;
        // `core.files` needs every source of what the host holds in use
        // (the library's move, the network's files, the reader's book):
        // without one "nothing is busy" would be wrong, so it is refused.
        if (wiring.hyphenation.is_some() && hosts.hyphen().is_some())
            || (wiring.files.is_some()
                && (hosts.files().is_some()
                    || !(hosts.busy_sources().complete() || wiring.documents.is_some())))
            || (wiring.documents.is_some() && hosts.documents().is_some())
            || (wiring.library.is_some() && hosts.library().is_some())
            || (wiring.lookup && hosts.lookup().is_some())
            || (wiring.device.is_some() && hosts.device().is_some())
            || (wiring.net.is_some() && hosts.net().is_some())
            || (wiring.sockets.is_some() && hosts.sockets().is_some())
            || (wiring.ui.is_some() && hosts.ui().is_some())
            || (wiring.secrets.is_some() && hosts.secrets().is_some())
            || (wiring.work.is_some() && hosts.work_wired())
        {
            return Err(ERR_STATE);
        }
        // `core.net` and `core.sockets` share the device's one network pool: a
        // wiring that names two shapes is refused before anything is wired.
        if let (Some(net), Some(sockets)) = (&wiring.net, &wiring.sockets)
            && (
                net.pool_shape().0,
                super::net_pool::NetPool::clamp_sessions(net.pool_shape().1),
            ) != (
                sockets.pool_shape().0,
                super::net_pool::NetPool::clamp_sessions(sockets.pool_shape().1),
            )
        {
            return Err(ERR_INVALID);
        }
        let mut wired = WiredHosts::default();
        self.set_plugin_heap(
            wiring
                .plugin_heap
                .unwrap_or_else(|| wiring.class_plugin_heap()),
            wiring.plugin_heap.is_some(),
        );
        if let Some(storage) = wiring.hyphenation {
            let tables = Rc::new(CardTables::new(storage));
            hosts.wire_hyphen(tables.clone())?;
            wired.hyphen = Some(HyphenTables::over(tables));
        }
        if let Some(host) = wiring.device {
            hosts.wire_device(host)?;
        }
        if let Some(inputs) = wiring.net {
            wired.net = Some(wire_net(hosts, inputs)?);
        }
        if let Some(inputs) = wiring.sockets {
            wired.sockets = Some(wire_sockets(hosts, inputs)?);
        }
        if let Some((memory, long_side)) = wiring.documents {
            wire_documents(
                hosts,
                memory,
                long_side,
                wiring.files.clone(),
                wiring.document_colour,
            )?;
        }
        let mut library_moves = None;
        if wiring.lookup {
            hosts.wire_lookup(
                Rc::new(super::lookup_service::Providers),
                super::device_log::DeviceLog::default(),
            )?;
        }
        if let Some(storage) = wiring.files {
            library_moves = Some(wire_files(hosts, storage)?);
        }
        // The library's transactions read the device's memory class and clock
        // from what any wiring told, whichever call it was in (the X4 shell
        // wires `ui`, `documents`, `work` and `library` in calls of their own).
        let policy = hosts.edit_policy();
        if wiring.ui == Some(true)
            || wiring
                .documents
                .is_some_and(|(memory, _)| super::ui_store::is_x4(memory))
            || wiring.work.as_ref().is_some_and(WorkInputs::small)
        {
            policy.set_small();
        }
        if let Some(inputs) = &wiring.work {
            policy.set_clock(inputs.device());
        }
        if let Some((_, long_side)) = wiring.documents {
            policy.set_long_side(long_side);
        }
        if let Some((storage, env)) = wiring.library {
            wire_library(hosts, policy, storage, env, library_moves)?;
        }
        if let Some(x4) = wiring.ui {
            wired.ui_skipped = wire_ui(hosts, x4)?;
        }
        if let Some(inputs) = wiring.secrets {
            wired.secrets = Some(wire_secrets_host(hosts, inputs)?);
        }
        if let Some(inputs) = wiring.work {
            wire_work(hosts, inputs)?;
        }
        Ok(wired)
    }
}

/// One family's wiring step each, not inlined: the large inputs each takes by
/// value live in the frame of its own step, not in `wire`'s (the X4's frame
/// limit).
type Hosts = super::host_services::HostServices;

#[inline(never)]
fn wire_net(hosts: &Hosts, inputs: NetInputs) -> Result<NetReports, i32> {
    let owners = hosts.owners()?.try_clone().ok_or(ERR_NO_MEMORY)?;
    let (bytes, sessions) = inputs.pool_shape();
    let pool = hosts.net_pool(bytes, sessions)?.clone();
    let device =
        Rc::new(inputs.device(pool, hosts.in_use().clone(), owners, hosts.policy().clone()));
    hosts.wire_net(device.clone())?;
    if device.cable().is_some() {
        hosts.wire_cable(device.clone())?;
    }
    Ok(NetReports(device))
}

#[inline(never)]
fn wire_sockets(hosts: &Hosts, inputs: SockInputs) -> Result<SockReports, i32> {
    let owners = hosts.owners()?.try_clone().ok_or(ERR_NO_MEMORY)?;
    let (bytes, sessions) = inputs.pool_shape();
    let pool = hosts.net_pool(bytes, sessions)?.clone();
    let device =
        Rc::new(inputs.device(pool, hosts.in_use().clone(), owners, hosts.policy().clone()));
    hosts.wire_sockets(device.clone())?;
    Ok(SockReports(device))
}

#[inline(never)]
fn wire_documents(
    hosts: &Hosts,
    memory: paperpaw_platform::MemoryClass,
    long_side: u32,
    files: Option<Rc<dyn Storage>>,
    colour: bool,
) -> Result<(), i32> {
    let limits = if super::ui_store::is_x4(memory) {
        super::doc_service::Limits::X4
    } else {
        super::doc_service::Limits::LARGE
    };
    let state = super::doc_host::DocState::try_new(limits, long_side, hosts.in_use().clone())?
        .with_storage(files)
        .with_colour(colour);
    // The reader's open book is a source of `core.files` from now on.
    hosts.register_reader_book_reporting()?;
    hosts.wire_documents(Rc::new(state))
}

/// The library's move job, a busy source `core.files` needs and the way a move
/// of the user's files goes; the same card holds the font files of the
/// plugins' data folders (`font.acquire` `data:<path>`). Gives the job.
#[inline(never)]
fn wire_files(
    hosts: &Hosts,
    storage: Rc<dyn Storage>,
) -> Result<Rc<super::lib_moves_service::LibraryMoves>, i32> {
    hosts.wire_font_card(storage.clone())?;
    let moves = hosts.wire_library_moves(storage.clone())?;
    hosts.wire_files(Rc::new(
        StorageFiles::new(
            storage,
            hosts.busy_sources().clone(),
            hosts.linked().clone(),
        )
        .with_moves(moves.clone())
        .with_data_plugins(hosts.data_plugins().clone()),
    ))?;
    Ok(moves)
}

/// After `core.files`, whose library move job it shares: a book the job
/// carries is in use for `provide` (also the job an earlier wiring's `files`
/// made).
#[inline(never)]
fn wire_library(
    hosts: &Hosts,
    policy: &Rc<super::lib_edits::EditPolicy>,
    storage: Rc<dyn Storage>,
    env: Rc<dyn CatalogEnv>,
    moves: Option<Rc<super::lib_moves_service::LibraryMoves>>,
) -> Result<(), i32> {
    let moves = moves.or_else(|| hosts.library_moves().cloned());
    hosts.wire_library(Rc::new(
        LibraryHost::new(storage, env, moves)
            .with_policy(policy.clone())
            .with_busy_sources(hosts.busy_sources().clone()),
    ))
}

/// A store that cannot be allocated degrades: no `core.ui` (a plugin asks and
/// is told the service is not there), the other families wired. True when it
/// degraded.
#[inline(never)]
fn wire_ui(hosts: &Hosts, x4: bool) -> Result<bool, i32> {
    let limits = if x4 { Limits::X4 } else { Limits::ELSEWHERE };
    match Shared::try_new(UiStore::new(limits)) {
        Ok(store) => hosts.wire_ui(UiServices::new(store, x4)).map(|()| false),
        Err(_) => Ok(true),
    }
}

#[inline(never)]
fn wire_secrets_host(hosts: &Hosts, inputs: SecretsInputs) -> Result<SecretsLifecycle, i32> {
    let host = inputs.host();
    hosts.wire_secrets(SecretsServices::new(host.clone()))?;
    Ok(SecretsLifecycle::new(host))
}

#[inline(never)]
fn wire_work(hosts: &Hosts, inputs: WorkInputs) -> Result<(), i32> {
    // The directory and the network policy are the device's from here on;
    // the scheduler and its view are made at the first call (#248).
    hosts.owners()?;
    hosts.policy();
    hosts.wire_work_later(inputs)
}

#[cfg(test)]
#[path = "shell_wiring_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "shell_wiring/net_tests.rs"]
mod net_tests;

#[cfg(test)]
#[path = "shell_wiring/sockets_tests.rs"]
mod sockets_tests;
