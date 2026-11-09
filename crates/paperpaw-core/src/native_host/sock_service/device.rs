//! The production [`SockHost`]: core's adapter over the device's storage and
//! the traits the shell implements ([`SockTransport`] for the bytes,
//! [`SockTls`] for the handshake, [`ShellNetwork`] for the clock, the network
//! and the power state, which `core.net` uses too).
//!
//! What core derives itself, as `core.net`'s adapter does ([`NetDevice`]): the
//! access to a root, the free-space reserve, the paths a read to a file holds
//! (in the device's one table, exclusively: no other writer, and no second
//! read of the same plugin), the staged file (`<path>.pp-staged`, core's one
//! name) and its commit, and the owner views of the kernel: subscriptions,
//! the work run of the running callback and the screen that is showing.
//!
//! [`NetDevice`]: crate::native_host::net_service::device::NetDevice
use super::{PowerEnd, SockHost, SockProfile, SockTable, SockTls, SockTransport, reason};
use crate::native_host::{
    host_services::Caller,
    net_policy::NetPolicy,
    net_pool::NetPool,
    owner_directory::OwnerDirectory,
    owner_paths::{self, OwnerPaths},
    paths_in_use::PathsInUse,
    save_files::SaveFiles,
    shared::Shared,
    shell_wiring::ShellNetwork,
    staged_name,
    storage::{self, has_room},
};
use alloc::{rc::Rc, string::String};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::Storage;

/// The device's `core.sockets` host.
pub(crate) struct SockDevice {
    table: SockTable,
    storage: Rc<dyn Storage>,
    transport: Rc<dyn SockTransport>,
    tls: Option<Rc<dyn SockTls>>,
    network: Rc<dyn ShellNetwork>,
    profile: SockProfile,
    saves: SaveFiles,
    paths: OwnerPaths,
    /// What the user allows (`net_policy`).
    policy: Rc<NetPolicy>,
}

/// The staged name of a target.
fn staged_of(target: &str) -> Result<String, i32> {
    staged_name::staged_path(target).ok_or(ERR_NO_MEMORY)
}

impl SockDevice {
    /// The host over `storage`, `transport` and `network`, charging `pool` and
    /// holding its file reads in `in_use`. Without `tls` the TLS ops are
    /// `ERR_UNSUPPORTED`.
    #[allow(clippy::too_many_arguments, reason = "one device, built in one place")]
    pub(crate) fn new(
        storage: Rc<dyn Storage>,
        transport: Rc<dyn SockTransport>,
        tls: Option<Rc<dyn SockTls>>,
        network: Rc<dyn ShellNetwork>,
        profile: SockProfile,
        pool: Rc<NetPool>,
        in_use: Rc<PathsInUse>,
        owners: Shared<OwnerDirectory>,
        policy: Rc<NetPolicy>,
    ) -> Self {
        Self {
            policy,
            table: SockTable::new(pool),
            saves: SaveFiles::new(storage.clone(), in_use),
            storage,
            transport,
            tls,
            network,
            profile,
            paths: OwnerPaths::new(owners),
        }
    }

    /// The joined target and its staged name of `root`/`path` of `owner`.
    fn names(&self, owner: u64, root: u32, path: &str) -> Result<(String, String), i32> {
        let target = self.paths.join_owned(owner, root, path)?;
        let staged = staged_of(&target)?;
        Ok((target, staged))
    }
}

impl SockHost for SockDevice {
    fn table(&self) -> &SockTable {
        &self.table
    }
    fn transport(&self) -> &dyn SockTransport {
        &*self.transport
    }
    fn tls(&self) -> Option<&dyn SockTls> {
        self.tls.as_deref()
    }
    fn profile(&self) -> SockProfile {
        self.profile
    }
    fn now_ms(&self) -> u64 {
        self.network.now_ms()
    }
    fn subscribed(&self, owner: u64, tag: u32) -> bool {
        self.paths.owners().subscribed(owner, tag)
    }

    fn power_end(&self, owner: u64, background: bool) -> Option<PowerEnd> {
        let mut end = None;
        match self
            .paths
            .plugin_of(owner, &mut |id| end = self.policy.end_for(id, background))
        {
            // A table in use is asked again at the next pass.
            Ok(()) | Err(ERR_BUSY) => end,
            // A plugin whose id is not known is not let out.
            Err(_) => Some(PowerEnd {
                reason: reason::NETWORK_OFF,
                later: false,
            }),
        }
    }

    fn network_up(&self) -> bool {
        self.network.network_up()
    }
    fn network_demand(&self, wanted: bool) {
        self.network.network_demand(wanted);
    }

    fn current_run(&self, owner: u64) -> Option<u64> {
        self.paths.owners().current_run(owner)
    }
    fn showing_screen(&self, owner: u64) -> Option<u64> {
        self.paths.owners().showing_screen(owner)
    }

    fn root_access(&self, who: &Caller<'_>, root: u32, path: &str) -> Result<(), i32> {
        owner_paths::access(who, root, path, true)
    }

    fn room_for(&self, extra: u64) -> bool {
        has_room(&*self.storage, extra)
    }

    fn claim_save(&self, owner: u64, root: u32, path: &str) -> Result<(), i32> {
        let (target, staged) = self.names(owner, root, path)?;
        // Exclusive: nobody holds the target or its staged file, the
        // plugin's own NET request included.
        self.saves.claim(owner, root, path, &target, &staged, true)
    }

    fn release_save(&self, owner: u64, root: u32, path: &str) {
        self.saves.release_at(owner, root, path);
    }

    fn recover_saves(&self, _owner: u64) {
        self.saves.recover(None);
    }

    fn staged_write(
        &self,
        owner: u64,
        root: u32,
        path: &str,
        data: &[u8],
        first: bool,
    ) -> Result<(), i32> {
        let (target, staged) = self.names(owner, root, path)?;
        if first {
            self.saves
                .begin(owner, &target, &staged)
                .map_err(storage::status)?;
        }
        self.saves.append(&staged, data).map_err(storage::status)
    }

    fn staged_commit(&self, owner: u64, root: u32, path: &str) -> Result<(), i32> {
        let (target, staged) = self.names(owner, root, path)?;
        self.saves
            .commit(owner, &target, &staged)
            .map_err(storage::status)
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
