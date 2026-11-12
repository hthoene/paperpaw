//! The kernel's pump for the events of host services (`core.net` 99-102,
//! `core.sockets` 106-107): between callbacks, each running plugin gets the
//! events its services queued, one callback each, and the service frees what
//! the callback could read when it returns.
//!
//! The pump runs at the end of every kernel call (`Kernel::finish`: a tick, an
//! input); the shell ticks after it reports (`NetReports`, `SockReports`). It never runs inside a
//! callback (events raised during one come after it returns), keeps each
//! service's order for one plugin, and delivers at most [`PER_OWNER`] events
//! of one service to one plugin in a pass so that no plugin starves another;
//! when events remain it asks for another pass at once. The services' clocks
//! (`timeout_ms`, `idle_ms`, `wait_ms`) are driven from the same tick, which
//! the kernel asks for at the first deadline of any of them (`service_due`).
use super::{
    native_instances::NativeInstances,
    service_events::{EVENT_BUFFER, ServiceEvents},
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_SPACE};
use paperpaw_platform::Instant;
use paperpaw_sdk::Env;

/// The wait before the pump tries again for its buffer after it was refused
/// (the buffer is made once, on the first pass with a service wired, and kept).
const RETRY_MS: u64 = 2_000;
/// What the buffer keeps between passes: nearly every event (a finished
/// request, a progress, a closed document) is smaller. The largest
/// ([`EVENT_BUFFER`], a served request) is made when an event asks for it
/// (`ERR_NO_SPACE`, which consumes nothing) and given back when the pass is
/// over, so the device holds 256 bytes, not 1,280, while it is idle.
const EVENT_SMALL: usize = 256;

/// Events of one service for one plugin in one pass.
const PER_OWNER: usize = 4;

/// One pass: the services' clocks, then the events of every running plugin.
/// `true` when events remain for another pass.
pub(super) fn pump(
    instances: &NativeInstances,
    services: &[Option<&dyn ServiceEvents>; 3],
    env: &Env<'_>,
    call: &dyn Fn(u64, &super::native_instances::NativeLease, &[u8]),
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
    let now = env.now().millis();
    for service in services.iter().flatten() {
        service.set_now(now);
    }
    // Short of memory: the events wait, and the next try is not before
    // `RETRY_MS`, so a device without the bytes does not run the pump in a loop.
    if instances.service_retry().is_some_and(|at| now < at) {
        return false;
    }
    let Ok(mut buffer) = instances.service_buffer().try_borrow_mut() else {
        return true;
    };
    if buffer.len() < EVENT_SMALL && !grow(&mut buffer, EVENT_SMALL) {
        instances.set_service_retry(Some(now.saturating_add(RETRY_MS)));
        return false;
    }
    instances.set_service_retry(None);
    let delivery = Delivery {
        sched: instances.work_services().map(|work| &*work.sched),
        instances,
        call,
        now,
    };
    let mut more = false;
    let running = (0..)
        .take_while(|&n| instances.running_nth(n).is_some())
        .count();
    // Each pass starts where the last one stopped for a staged batch, so a
    // plugin whose every event stages commands does not keep the others
    // waiting.
    let start = instances.service_cursor().checked_rem(running).unwrap_or(0);
    for step in 0..running {
        let index = (start + step) % running;
        let Some((key, lease)) = instances.running_nth(index) else {
            break;
        };
        for service in services.iter().flatten() {
            more |= drain(*service, (key, lease), &mut buffer, &delivery);
            if instances.outbox.full() {
                break;
            }
        }
        // A callback's commands wait: none is called before they ran.
        if instances.outbox.full() {
            instances.set_service_cursor(index + 1);
            return true;
        }
    }
    instances.set_service_cursor(0);
    // What the largest event asked for is given back with the pass.
    if buffer.len() > EVENT_SMALL {
        buffer.truncate(EVENT_SMALL);
        buffer.shrink_to(EVENT_SMALL);
    }
    more
}

/// What one pass hands each event to.
#[derive(Clone, Copy)]
struct Delivery<'a> {
    sched: Option<&'a super::work_sched::WorkSched>,
    instances: &'a NativeInstances,
    call: &'a dyn Fn(u64, &super::native_instances::NativeLease, &[u8]),
    now: u64,
}

/// Makes the buffer `size` bytes (fallibly); `false` where it is refused.
fn grow(buffer: &mut alloc::vec::Vec<u8>, size: usize) -> bool {
    let wanted = size.saturating_sub(buffer.len());
    if super::net_service::gate().is_err() || buffer.try_reserve_exact(wanted).is_err() {
        return false;
    }
    buffer.resize(size, 0);
    true
}

/// Whether `owner` subscribed to the tag of the one-record event `event`.
fn subscribed(instances: &NativeInstances, owner: u64, event: &[u8]) -> bool {
    super::registrations::RawRecord::single(event)
        .is_ok_and(|raw| instances.subscribed(owner, raw.tag()))
}

/// Up to [`PER_OWNER`] events of `service` for `owner`; `true` when more may
/// wait (the cap was reached, or the table was busy).
fn drain(
    service: &dyn ServiceEvents,
    (owner, lease): (u64, &super::native_instances::NativeLease),
    buffer: &mut alloc::vec::Vec<u8>,
    delivery: &Delivery<'_>,
) -> bool {
    let Delivery {
        sched,
        instances,
        call,
        now,
    } = *delivery;
    let mut taken = 0;
    while taken < PER_OWNER {
        match service.next_event(owner, buffer) {
            Ok(Some((len, run))) => {
                // The events of a work run's request or socket come in the
                // run (background, its requests tied to it); the run counts
                // them until their callbacks returned (`work.md` 2.3).
                let _guard = sched
                    .zip(run.and_then(|run| u32::try_from(run).ok()))
                    .map(|(sched, run)| sched.enter_run_callback(owner, run));
                // A failing callback does not get the event again: the
                // service frees what was readable either way. Where the
                // service tells subscribers only, a plugin that did not
                // subscribe to the event's tag is not called (its dispatch
                // would refuse it unread): the event is dropped, as the
                // picture pump does.
                if !service.subscribers_only() || subscribed(instances, owner, &buffer[..len]) {
                    call(owner, lease, &buffer[..len]);
                }
                drop(_guard);
                if service.event_delivered(owner) == Err(ERR_BUSY) {
                    return true;
                }
                // What it staged runs before the next callback.
                if instances.outbox.full() {
                    return true;
                }
            }
            Ok(None) => return false,
            // The event is larger than the buffer kept: it is made larger
            // for this pass; where that is refused the event waits.
            Err(ERR_NO_SPACE) if buffer.len() < EVENT_BUFFER => {
                if !grow(buffer, EVENT_BUFFER) {
                    instances.set_service_retry(Some(now.saturating_add(RETRY_MS)));
                    return false;
                }
                continue;
            }
            Err(status) => return status == ERR_BUSY,
        }
        taken += 1;
    }
    true
}

impl crate::Kernel {
    /// The services' events of [`Self::native_service_pump`], each to its
    /// subscriber; `true` where more wait. Its own frame: the environment is
    /// gone before the other pumps call plugins (#250).
    #[inline(never)]
    fn native_event_services(&mut self) -> bool {
        let env = self.shared.env(self.now);
        let instances = &self.shared.native_instances;
        pump(
            instances,
            &instances.event_services(),
            &env,
            // The callback gets its command stage; its batch is run below.
            &|owner, lease, bytes| {
                let called = self.native_event_call(owner, lease, bytes);
                debug_assert!(
                    called != Err(ERR_BUSY),
                    "the outbox is empty before each event is called"
                );
            },
        )
    }

    /// Runs the services' clocks and delivers their events (see the module).
    // Its own frame: not inlined into `Kernel::finish` (#250).
    #[inline(never)]
    pub(crate) fn native_service_pump(&mut self) {
        // What the user decided about the network, before the services'
        // clocks run: a plugin whose network was turned off loses what it
        // has open in this pass.
        self.sync_net_policy();
        self.shared
            .native_instances
            .retry_run_closes(self.now.millis());
        // A batch left waiting (a callback outside the pumps staged it) runs
        // first: no event is called while one waits, and none is lost for it.
        // Never inside a callback (as the work and picture pumps): the pump
        // below asks for another pass instead.
        if self.shared.native_instances.depth() == 0 {
            self.native_outbox_run();
        }
        let more = self.native_event_services();
        if self.shared.native_instances.depth() == 0 {
            self.native_outbox_run();
        }
        // Declared work comes due on the same tick (`work_pump`).
        let work_more = self.native_work_pump();
        // Pictures a plugin asked for decode one a turn (`image_pump`).
        let image_more = self.native_image_pump();
        // The reading session's events are queued for the subscribers (the
        // reader's report raised them), before the pump that tells them.
        let reading_more = self.native_reading_pump();
        // The native formats' files are books of the walk (`native_book_formats`).
        self.native_book_formats_sync();
        // What the library says changed is queued for the subscribers
        // (`library_events`), before the pump that tells them.
        let library_more = self.native_library_pump();
        // How native plugins' texts ended is queued the same way (`speech`).
        let speech_more = self.native_speech_pump();
        // Changed settings reach the plugins that subscribed (`setting_events`).
        let setting_more = self.native_setting_pump();
        // Widgets their owners said changed reach their makers (`widget_events`).
        let widget_more = self.native_widget_pump();
        // A changed screen stack reaches the plugins that subscribed (`stack_events`).
        let stack_more = self.native_stack_pump();
        // The folders of the running data plugins follow the plugin set.
        self.native_data_plugins_sync();
        // The library's move job goes on, a bounded step a pass (`moves_pump`).
        let moves_more = self.native_moves_pump();
        // The cover job waits while a library list is up (`doc_cover_job`).
        self.native_cover_list();
        // A document a format is preparing takes a step (`doc_pump`).
        let doc_more = self.native_doc_pump();
        // A cover job whose preparation is over ends here (`doc_cover_job`).
        self.native_cover_job_pump();
        // The host's details job begins on the next native book whose details
        // are unread (`details_job_pump`); never "more", a timed wake at most.
        self.native_details_job_pump();
        // Questions open their dialogs and their answers come back.
        let dialog_more = self.native_dialog_pump();
        // The outcomes of Wi-Fi requests reach their senders (`wifi_events`).
        let wifi_more = self.native_wifi_pump();
        // A firmware install or check takes a step and tells its plugin
        // (`firmware_events`).
        let firmware_more = self.native_firmware_pump();
        self.shared.native_instances.set_service_more(
            more || work_more
                || image_more
                || reading_more
                || setting_more
                || widget_more
                || stack_more
                || library_more
                || speech_more
                || moves_more
                || doc_more
                || dialog_more
                || wifi_more
                || firmware_more,
        );
        // Saved passwords that did not open here: said once per start.
        self.secrets_notice();
    }

    /// When the pump wants to run again: now while events remain, else at
    /// the first real deadline of the services (`timeout_ms`, `idle_ms`,
    /// `wait_ms`, the waits for a network or a TLS session), in the shell's
    /// clock. Nothing is polled: what the shell reports reaches the pump
    /// through its tick at once, so an idle socket costs no wake-up.
    pub(crate) fn service_due(&self) -> Option<Instant> {
        let instances = &self.shared.native_instances;
        // An event queued after this call's pump ran (the commands that ran
        // after it raised it) is told at once too.
        if instances.service_more() || instances.setting_events.any_waiting() {
            return Some(self.now);
        }
        instances
            .event_services()
            .iter()
            .flatten()
            .filter_map(|service| service.next_deadline())
            .chain(instances.service_retry())
            .chain(instances.image_pump.retry_at())
            .chain(instances.moves_retry())
            .chain(instances.doc_retry())
            .chain(self.native_details_due())
            .chain(self.native_demand_due().map(|at| at.millis()))
            .chain(self.native_wifi_due())
            .chain(self.firmware_due())
            .min()
            .map(Instant)
    }

    /// A screen instance closed: what each service started for it ends.
    /// Run again on `ERR_BUSY` (a retry repeats every step; each is idempotent).
    ///
    /// # Errors
    /// The first service's refusal, after the others were told.
    pub(crate) fn native_services_screen_closed(&self, instance: u32) -> Result<(), i32> {
        self.native_services_each(|service, owner| {
            service.screen_closed(owner, u64::from(instance))
        })
    }

    /// The plugin `owner` was taken in: what each service holds of a power
    /// cut's leftovers for it is cleaned up or restored (a `.received/`
    /// folder, a staged file kept after a failed rename). Nothing is
    /// reported: what cannot be done now is retried at the next claim or the
    /// plugin's stop.
    pub(crate) fn native_services_owner_started(&self, owner: u64) {
        tell_owner_started(&self.shared.native_instances.event_services(), owner);
    }

    /// A work run reached its longest time: what each service started in it
    /// ends. The work pump (`work_pump`) calls it where it takes an expired run.
    ///
    /// # Errors
    /// The first service's refusal, after the others were told.
    pub(crate) fn native_services_work_ended(&self, owner: u64, run: u64) -> Result<(), i32> {
        let services = self.shared.native_instances.event_services();
        let mut result = Ok(());
        for service in services.iter().flatten() {
            result = result.and(service.work_ended(owner, run));
        }
        result
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

/// Tells every wired service that `owner` started.
pub(super) fn tell_owner_started(services: &[Option<&dyn ServiceEvents>], owner: u64) {
    for service in services.iter().flatten() {
        service.owner_started(owner);
    }
}

#[cfg(test)]
#[path = "service_pump_tests.rs"]
pub(crate) mod tests;
