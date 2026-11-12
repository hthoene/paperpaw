//! The kernel's pump for `core.work@1` (`docs/abi/work.md` sections 2 and 3):
//! between callbacks it feeds the scheduler the device's moments, starts a
//! pass, delivers `event.work_refused` (104) and `event.work_due` (103) to
//! the plugins that can take them, tells the plugins whose work left a hold
//! `event.work_allowed` (105), and ends the runs that reached their longest
//! time.
//!
//! It runs at the end of every kernel call (`Kernel::finish`), never inside a
//! callback, and calls no plugin more than [`PER_PASS`] times in one pass;
//! when work remains it asks for another pass at once. The scheduler decides
//! what is due and what holds it; this module only carries it to plugins:
//!
//! * the events go through the plugin's event registration for their tag
//!   (`service_event`: the work's own token is in the record);
//! * a `work_due` callback runs under the run's guard (`enter_run_callback`:
//!   UI questions refused, `core.net` requests tied to the run) and the run's
//!   callback is over when it returns, whatever it returned;
//! * a refusal or a callback that failed is not told again: the entry ended
//!   with the call;
//! * a plugin that is not running, or did not subscribe to 103, is not
//!   called (lazy start does not apply to events); its work waits for it to
//!   run, or is dropped with it when it stops.
use super::{
    native_instances::InstanceState,
    registrations::{self as wire, Record},
    work_sched::{Event, Moment},
};
use crate::native_host::codec;
use alloc::rc::Rc;
use paperpaw_platform::Instant;

/// Plugin calls (events 103 to 105) in one pass: the most one device has
/// works (X4: 32), so one pass is the whole table and no more.
const PER_PASS: usize = 32;

/// The room of one record of 103 to 105: a 64-byte key and the fixed fields,
/// well under it.
const RECORD_BYTES: usize = 128;

const SEEN: u8 = 1;
const SEEN_UP: u8 = 1 << 1;
const SEEN_CHARGING: u8 = 1 << 2;

impl crate::Kernel {
    /// The device woke (`Kernel::resume`): work that asked for the `wake`
    /// moment may come due in the next pass.
    pub(crate) fn native_work_woke(&self) {
        if let Some(work) = self.shared.native_instances.work_services() {
            work.sched.moment(Moment::Wake, &*work.env);
        }
    }

    /// One pass of the work scheduler (see the module). `true` when plugin
    /// calls remain for another pass.
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_work_pump(&mut self) -> bool {
        // A batch left waiting runs first, so no entry is taken for a callback
        // that is then refused.
        if self.shared.native_instances.depth() == 0 {
            self.native_outbox_run();
        }
        let instances = &self.shared.native_instances;
        // The scheduler and its view of the device are held here, so that a
        // callback's commands can run (`&mut self`) between two calls.
        let Some((sched, env)) = instances
            .work_services()
            .map(|work| (Rc::clone(&work.sched), Rc::clone(&work.env)))
        else {
            return false;
        };
        // Never inside a callback: what comes due during one comes after.
        if instances.depth() > 0 {
            return true;
        }
        let env = &*env;
        self.tell_work_env(env);
        let now_ms = env.now_ms();
        let charging = self
            .shared
            .env(self.now)
            .status()
            .battery
            .is_some_and(|battery| battery.charging);
        let seen = instances.work_seen();
        // The first pass only looks: a network already up or a battery
        // already charging is no moment.
        if seen & SEEN != 0 {
            if env.network_up() && seen & SEEN_UP == 0 {
                sched.moment(Moment::Online, env);
            }
            if charging && seen & SEEN_CHARGING == 0 {
                sched.moment(Moment::Charging, env);
            }
        }
        instances.set_work_seen(
            SEEN | if env.network_up() { SEEN_UP } else { 0 }
                | if charging { SEEN_CHARGING } else { 0 },
        );
        // A table in use waits for the next pass.
        let Ok(pass) = sched.begin_pass(env) else {
            return true;
        };
        // The hints (one per subscribed plugin) are not work: they do not
        // count against the pass, so due work never waits behind them with
        // no wake to come back for it.
        if pass.allowed {
            self.work_tell_allowed(now_ms);
        }
        let mut calls = 0;
        let mut more = false;
        while calls < PER_PASS {
            let instances = &self.shared.native_instances;
            let accepts = |owner: u64| {
                instances.subscribed(owner, wire::EVENT_WORK_DUE_TAG)
                    && matches!(instances.state(owner), Ok(InstanceState::Running(_)))
            };
            let Ok(Some(event)) = sched.next_event(env, &accepts) else {
                break;
            };
            self.work_deliver(&event, now_ms);
            calls += 1;
            more |= calls == PER_PASS;
        }
        self.work_end_expired(now_ms);
        more
    }

    /// `event.work_allowed` (105) to every running plugin that subscribed: a
    /// hint, one per pass, so one pending at most.
    fn work_tell_allowed(&mut self, now_ms: u64) {
        let mut record = [0u8; RECORD_BYTES];
        let allowed = Record::EventWorkAllowed(wire::EventWorkAllowed { now_ms });
        let Ok(len) = codec::encode_batch(&[allowed], &mut record) else {
            debug_assert!(
                false,
                "an event.work_allowed record fits {RECORD_BYTES} bytes"
            );
            return;
        };
        let mut index = 0;
        loop {
            let instances = &self.shared.native_instances;
            let Some((owner, lease)) = instances.running_nth(index) else {
                break;
            };
            index += 1;
            let Ok(lease) = lease.try_clone() else {
                continue;
            };
            if instances.subscribed(owner, wire::EVENT_WORK_ALLOWED_TAG) {
                // A hint: how the plugin took it is its own; what it staged
                // runs before the next callback.
                let _ = self.native_event_call(owner, &lease, &record[..len]);
                self.native_outbox_run();
            }
        }
    }

    /// One event of the scheduler to its plugin.
    fn work_deliver(&mut self, event: &Event, now_ms: u64) {
        let instances = &self.shared.native_instances;
        let Some(sched) = instances.work_services().map(|work| Rc::clone(&work.sched)) else {
            return;
        };
        let (owner, token, run) = match event {
            Event::Due(due) => (due.owner, due.token, Some(due.run)),
            Event::Refused(refused) => (refused.owner, refused.token, None),
        };
        let mut record = [0u8; RECORD_BYTES];
        let encoded = match event {
            Event::Due(due) => codec::encode_batch(
                &[Record::EventWorkDue(wire::EventWorkDue {
                    now_ms,
                    token,
                    why: due.why,
                    key: due.key.as_str(),
                })],
                &mut record,
            ),
            Event::Refused(refused) => codec::encode_batch(
                &[Record::EventWorkRefused(wire::EventWorkRefused {
                    now_ms,
                    token,
                    reason: refused.reason,
                    key: refused.key.as_str(),
                })],
                &mut record,
            ),
        };
        debug_assert!(
            encoded.is_ok(),
            "an event.work_due record fits {RECORD_BYTES} bytes"
        );
        // A refusal goes only to a plugin that subscribed to 104.
        let told = encoded.is_ok()
            && (run.is_some() || instances.subscribed(owner, wire::EVENT_WORK_REFUSED_TAG));
        if let (true, Ok(len), Ok(InstanceState::Running(lease))) =
            (told, encoded, instances.state(owner))
        {
            // The outcome is the plugin's own: the entry ended with the call.
            let guard = run.map(|run| sched.enter_run_callback(owner, run));
            let _ = self.native_event_call(owner, &lease, &record[..len]);
            drop(guard);
            // What it staged runs before the next callback.
            self.native_outbox_run();
        }
        if let Some(run) = run {
            sched.callback_returned(run, now_ms);
        }
    }

    /// Runs that reached their longest time end with what they started.
    fn work_end_expired(&self, now_ms: u64) {
        let Some(work) = self.shared.native_instances.work_services() else {
            return;
        };
        while let Some(run) = work.sched.take_expired(now_ms) {
            // A service whose table is borrowed cannot be, outside a
            // callback: nothing to retry.
            let _ = self.native_services_work_ended(run.owner, u64::from(run.id));
            // What they ended counts until its end event was delivered.
            work.sched.expired_told(run.id, now_ms);
        }
        // What a late end event's callback started in a run that is over ends
        // at once.
        while let Some((owner, run)) = work.sched.take_late() {
            let _ = self.native_services_work_ended(owner, u64::from(run));
        }
        // A service that never delivered the end events: the run is over.
        while let Some(run) = work.sched.take_stuck(now_ms) {
            work.sched.end_run(run.id, now_ms);
        }
        // What an end-event callback started in a run past its longest time
        // ends at once, with the rest.
        work.sched.each_ending(&mut |owner, run| {
            let _ = self.native_services_work_ended(owner, u64::from(run));
        });
    }

    /// When the pump wants to run again: at once while plugin calls remain,
    /// else at the first time work can come due or a run must end (the
    /// shell's clock, which is the kernel's tick clock).
    pub(crate) fn work_pump_due(&self) -> Option<Instant> {
        let work = self.shared.native_instances.work_services()?;
        self.tell_work_env(&*work.env);
        work.sched.next_wake(&*work.env).map(Instant)
    }

    /// Tells the device's view what the kernel holds: the power level and
    /// stretch of the user's mode, and whether a real-time screen (one that
    /// takes raw keys or runs at a pace, not a native one) is on top.
    pub(crate) fn tell_work_env(&self, env: &dyn super::work_sched::WorkEnv) {
        use paperpaw_sdk::power::PowerLevel;
        let power = &self.shared.power;
        let level = match power.level {
            PowerLevel::Saving => 1,
            PowerLevel::Offline => 2,
            // Normal, and any level to come: counts as normal.
            _ => 0,
        };
        env.kernel_state(
            level,
            power.stretch,
            self.top_realtime() || self.top_mode().is_paced(),
        );
    }
}

#[cfg(test)]
#[path = "work_pump_tests.rs"]
mod tests;
