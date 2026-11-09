//! The kernel's pump for the events of host services (`core.net` 99-102,
//! `core.sockets` 106-107): between callbacks, each running plugin gets the
//! events its services queued, one callback each, and the service frees what
//! the callback could read when it returns.
//!
//! The pump runs from [`Kernel::tick`](crate::Kernel::tick); the shell ticks
//! after it reports (`NetReports`, `SockReports`). It never runs inside a
//! callback (events raised during one come after it returns), keeps each
//! service's order for one plugin, and delivers at most [`PER_OWNER`] events
//! of one service to one plugin in a pass so that no plugin starves another;
//! when events remain it asks for another pass at once. The services' clocks
//! (`timeout_ms`, `idle_ms`, `wait_ms`) are driven from the same tick, which
//! the kernel asks for at the first deadline of any of them (`service_due`).
use super::{native_instances::NativeInstances, service_events::{EVENT_BUFFER, ServiceEvents}};
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_platform::Instant;
use paperpaw_sdk::Env;

/// Events of one service for one plugin in one pass.
const PER_OWNER: usize = 4;

/// One pass: the services' clocks, then the events of every running plugin.
/// `true` when events remain for another pass.
fn pump(
    instances: &NativeInstances,
    services: &[Option<&dyn ServiceEvents>; 2],
    env: &Env<'_>,
) -> bool {
    for service in services.iter().flatten() {
        // A borrowed table waits for the next pass.
        let _ = service.tick();
    }
    if services.iter().all(Option::is_none) {
        return false;
    }
    // Never inside a callback: its events come after it returns.
    if instances.depth() > 0 {
        return true;
    }
    let mut buffer: Vec<u8> = Vec::new();
    if buffer.try_reserve_exact(EVENT_BUFFER).is_err() {
        return true;
    }
    buffer.resize(EVENT_BUFFER, 0);
    let mut more = false;
    let mut index = 0;
    while let Some((key, lease)) = instances.running_nth(index) {
        index += 1;
        for service in services.iter().flatten() {
            more |= drain(*service, key, lease, &mut buffer, env);
        }
    }
    more
}

/// Up to [`PER_OWNER`] events of `service` for `owner`; `true` when more may
/// wait (the cap was reached, or the table was busy).
fn drain(
    service: &dyn ServiceEvents,
    owner: u64,
    lease: &super::native_instances::NativeLease,
    buffer: &mut [u8],
    env: &Env<'_>,
) -> bool {
    for _ in 0..PER_OWNER {
        match service.next_event(owner, buffer) {
            Ok(Some(len)) => {
                // A failing callback does not get the event again: the
                // service frees what was readable either way.
                let _ = lease.service_event(&buffer[..len], env);
                if service.event_delivered(owner) == Err(ERR_BUSY) {
                    return true;
                }
            }
            Ok(None) => return false,
            Err(status) => return status == ERR_BUSY,
        }
    }
    true
}

impl crate::Kernel {
    /// Runs the services' clocks and delivers their events (see the module).
    pub(crate) fn native_service_pump(&mut self) {
        let env = self.shared.env(self.now);
        let instances = &self.shared.native_instances;
        let more = pump(instances, &instances.event_services(), &env);
        instances.set_service_more(more);
    }

    /// When the pump wants to run again: now while events remain, else at
    /// the first real deadline of the services (`timeout_ms`, `idle_ms`,
    /// `wait_ms`, the waits for a network or a TLS session), in the shell's
    /// clock. Nothing is polled: what the shell reports reaches the pump
    /// through its tick at once, so an idle socket costs no wake-up.
    pub(crate) fn service_due(&self) -> Option<Instant> {
        let instances = &self.shared.native_instances;
        if instances.service_more() {
            return Some(self.now);
        }
        instances
            .event_services()
            .iter()
            .flatten()
            .filter_map(|service| service.next_deadline())
            .min()
            .map(Instant)
    }

    /// A screen instance closed: what each service started for it ends.
    /// Run again on `ERR_BUSY` (a retry repeats every step; each is idempotent).
    ///
    /// # Errors
    /// The first service's refusal, after the others were told.
    pub(crate) fn native_services_screen_closed(&self, instance: u32) -> Result<(), i32> {
        self.native_services_each(|service, owner| service.screen_closed(owner, u64::from(instance)))
    }

    /// A work run reached its longest time: what each service started in it
    /// ends. The work scheduler's pump calls it where it takes an expired run.
    ///
    /// # Errors
    /// The first service's refusal, after the others were told.
    #[allow(dead_code, reason = "the work scheduler has no pump on the kernel yet")]
    pub(crate) fn native_services_work_ended(&self, owner: u64, run: u64) -> Result<(), i32> {
        let services = self.shared.native_instances.event_services();
        services
            .iter()
            .flatten()
            .fold(Ok(()), |result, service| result.and(service.work_ended(owner, run)))
    }

    /// `each` for every running plugin and every wired service.
    fn native_services_each(
        &self,
        each: impl Fn(&dyn ServiceEvents, u64) -> Result<(), i32>,
    ) -> Result<(), i32> {
        let instances = &self.shared.native_instances;
        let services = instances.event_services();
        let mut result = Ok(());
        let mut index = 0;
        while let Some((owner, _)) = instances.running_nth(index) {
            index += 1;
            for service in services.iter().flatten() {
                result = result.and(each(*service, owner));
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "service_pump_tests.rs"]
mod tests;
