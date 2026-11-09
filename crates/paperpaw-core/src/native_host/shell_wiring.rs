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
//! `core.net` (the card and the shell's network) today; the other families'
//! hosts are core's adapters over kernel objects and join as their adapters
//! land, one method each.
use super::{
    device_service::DeviceHost,
    hyphen_card::CardTables,
    resources::HostResources,
    shared::Shared,
    storage_files::StorageFiles,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
#[path = "shell_wiring/net.rs"]
mod net;
#[path = "shell_wiring/sockets.rs"]
mod sockets;
pub use super::sock_service::{Connect, PeerInfo, SockTls, SockTransport, TlsOpen, TlsPin};
use alloc::{rc::Rc, vec::Vec};
pub use net::{
    HttpDone, HttpHeaders, HttpJob, HttpReason, NetInputs, NetLimits, NetReports, ServedDecision,
    ServedReply, ServerReason, ShellHttp, ShellNetwork,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_platform::Storage;
pub use sockets::{SockFailure, SockInputs, SockLimits, SockReports, TlsFailure, UntrustedWhy};

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
            .finish()
    }
}

impl HostWiring {
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
                && (hosts.files().is_some() || !hosts.busy_sources().complete()))
            || (wiring.device.is_some() && hosts.device().is_some())
            || (wiring.net.is_some() && hosts.net().is_some())
            || (wiring.sockets.is_some() && hosts.sockets().is_some())
            || (wiring.ui.is_some() && hosts.ui().is_some())
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
        if let Some(storage) = wiring.hyphenation {
            let tables = Rc::new(CardTables::new(storage));
            hosts.wire_hyphen(tables.clone())?;
            wired.hyphen = Some(HyphenTables::over(tables));
        }
        if let Some(host) = wiring.device {
            hosts.wire_device(host)?;
        }
        if let Some(inputs) = wiring.net {
            let owners = hosts.owners()?.try_clone().ok_or(ERR_NO_MEMORY)?;
            let (bytes, sessions) = inputs.pool_shape();
            let pool = hosts.net_pool(bytes, sessions)?.clone();
            let device = Rc::new(inputs.device(
                pool,
                hosts.in_use().clone(),
                owners,
                hosts.policy().clone(),
            ));
            hosts.wire_net(device.clone())?;
            if device.cable().is_some() {
                hosts.wire_cable(device.clone())?;
            }
            wired.net = Some(NetReports(device));
        }
        if let Some(inputs) = wiring.sockets {
            let owners = hosts.owners()?.try_clone().ok_or(ERR_NO_MEMORY)?;
            let (bytes, sessions) = inputs.pool_shape();
            let pool = hosts.net_pool(bytes, sessions)?.clone();
            let device = Rc::new(inputs.device(
                pool,
                hosts.in_use().clone(),
                owners,
                hosts.policy().clone(),
            ));
            hosts.wire_sockets(device.clone())?;
            wired.sockets = Some(SockReports(device));
        }
        if let Some(storage) = wiring.files {
            hosts.wire_files(Rc::new(StorageFiles::new(
                storage,
                hosts.busy_sources().clone(),
                hosts.linked().clone(),
            )))?;
        }
        if let Some(x4) = wiring.ui {
            let limits = if x4 { Limits::X4 } else { Limits::ELSEWHERE };
            // A store that cannot be allocated degrades: no `core.ui` (a plugin
            // asks and is told the service is not there), the other families
            // wired; the shell is told ([`WiredHosts::ui_skipped`]).
            match Shared::try_new(UiStore::new(limits)) {
                Ok(store) => hosts.wire_ui(UiServices::new(store, x4))?,
                Err(_) => wired.ui_skipped = true,
            }
        }
        Ok(wired)
    }
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
