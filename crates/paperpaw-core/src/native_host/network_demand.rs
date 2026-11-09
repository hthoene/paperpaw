//! The demand for the network of the native services, as the network
//! manager hears it (`device.md` 3): `event.network_wanted` when something
//! waits for a network that is not up, `event.network_idle` when the last
//! request, server or socket is gone, and the flags the shell reads
//! (`Env::wifi`: `wanted`, `busy`).
//!
//! The events and flags are the kernel's own (`Kernel::check_network`, after
//! every call, where PaperPaw manages Wi-Fi); this adds what `core.net` and
//! `core.sockets` want, behind the power mode and each plugin's own network
//! setting: a request or socket that policy refuses ends before it is stored
//! as live, so a plugin that is off or held creates no demand. The work
//! scheduler adds a one-shot wish ([`Kernel::native_work_wish`]) that stands
//! for [`WORK_WISH_MS`] and lapses when a network is up.
//!
//! The shell's `network_demand` is told the combined edge as before (once per
//! change, from the services' tables and from the wish).
use super::net_pool::WORK_WISH_MS;
use paperpaw_platform::Instant;

impl crate::Kernel {
    /// Whether a network is up (or the system manages it).
    fn native_online(&self) -> bool {
        self.network_ready()
    }

    /// What the native services want of the network now: whether anything
    /// waits for or uses it (`wants`) and whether a request, server or socket
    /// is live (`live`). A wish of the work scheduler that has lapsed (its
    /// time is out, or a network is up) goes first.
    pub(crate) fn native_demand(&mut self) -> (bool, bool) {
        let (now, online) = (self.now.millis(), self.native_online());
        let Some((pool, hosts)) = self.shared.native_instances.net_demand() else {
            return (false, false);
        };
        if let Some(edge) = pool.work_lapse(now, online) {
            hosts.tell_demand(edge);
        }
        (pool.wants(), pool.live())
    }

    /// The work scheduler wishes for a network: its network work's time or
    /// deadline came while none is up. A one-shot: the demand stands for
    /// [`WORK_WISH_MS`], long enough for the network manager to bring one up
    /// and for the work to ask for it itself, and lapses then or when one is
    /// up. The network manager hears `event.network_wanted` at the end of this
    /// call. Nothing without `core.net` or `core.sockets` wired.
    #[allow(dead_code, reason = "the work pump calls it (todo 265)")]
    pub(crate) fn native_work_wish(&mut self) {
        let now = self.now.millis();
        let Some((pool, hosts)) = self.shared.native_instances.net_demand() else {
            return;
        };
        if let Some(edge) = pool.work_wish(now) {
            hosts.tell_demand(edge);
        }
    }

    /// When the wish of work lapses, for the kernel's wake-up.
    pub(crate) fn native_demand_due(&self) -> Option<Instant> {
        let (pool, _) = self.shared.native_instances.net_demand()?;
        pool.work_deadline().map(Instant)
    }
}

const _: () = assert!(
    WORK_WISH_MS >= 1_000,
    "a wish outlives the call that made it"
);
