//! The firmware pump of the kernel: one bounded step of the firmware table's
//! open job from the main loop, and the events it raises for the plugin that
//! asked (`docs/abi/device.md` 4, 5.3): `event.firmware_changed` (96) as the
//! install advances and `event.install_done` (97) once per request.
//!
//! * **Never inside a callback.** The pump does nothing while a native
//!   callback is on the stack, and a batch a callback staged runs before the
//!   next callback is called.
//! * **96 goes to the plugin whose request it is**, at most every 250 ms while
//!   it writes (the machine's own gate) and at each change of its own state; any
//!   plugin reads the state with op 3. A plugin that did not subscribe is not
//!   told.
//! * **97 is never dropped.** Its record waits in the table, in room reserved
//!   when the request was accepted, until its plugin can be called; it is
//!   marked delivered (and readable with op 13) once the callback was called,
//!   whatever it answered, or at once for a plugin that stopped or did not
//!   subscribe. An owner whose callback cannot be called now (a batch of its
//!   waits, or it is busy) is told on a later pass.
use super::{
    codec,
    device_service::DeviceBuiltIns,
    firmware_install::{BuiltIns, KIND_CHECK, Outcome},
    native_instances::InstanceState,
    registrations::{self as wire, Record},
};
use crate::Kernel;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_STATE,
    ERR_UNSUPPORTED, OK,
};

/// The room of one record of 96 or 97 (a batch of one).
const RECORD_BYTES: usize = 128;
/// Outcomes told in one pass.
const PER_PASS: usize = 4;

/// No built-ins are known (no device host wired): none are named.
struct NoBuiltIns;
impl BuiltIns for NoBuiltIns {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

/// `install_done.status` of a reason (`device.md` 5.5). A check that ran ends
/// `OK` with its verdict as the reason; only a check that could not run (other,
/// card error, not on this device, stopped) carries the error.
#[must_use]
pub(crate) fn status_of(outcome: Outcome) -> i32 {
    let by_reason = match outcome.reason {
        0 => OK,
        2 | 5..=9 | 15 => ERR_DENIED,
        3 | 20 => ERR_CANCELLED,
        4 => ERR_INVALID,
        10..=12 | 14 | 19 => ERR_UNSUPPORTED,
        13 | 17 => ERR_NO_SPACE,
        18 | 21 => ERR_STATE,
        _ => ERR_IO,
    };
    if outcome.kind == KIND_CHECK && !matches!(outcome.reason, 1 | 16 | 19 | 20) {
        OK
    } else {
        by_reason
    }
}

impl Kernel {
    /// One step of the firmware table's job, then the outcomes that wait for
    /// their plugin. `true` while something is left (another pass).
    pub(crate) fn native_firmware_pump(&mut self) -> bool {
        let Some(driver) = self.firmware.clone() else {
            return false;
        };
        if self.shared.native_instances.depth() > 0 {
            return driver.try_borrow().map_or(true, |driver| driver.has_work());
        }
        self.firmware_wire(&driver);
        self.native_outbox_run();
        {
            let Ok(mut driver) = driver.try_borrow_mut() else {
                return true;
            };
            let now = self.now.millis();
            let hosts = self.shared.native_instances.hosts();
            match hosts.and_then(|hosts| hosts.device()) {
                Some(device) => driver.pump(self.storage(), &DeviceBuiltIns(&*device.0), now),
                None => driver.pump(self.storage(), &NoBuiltIns, now),
            };
        }
        // Progress only marked the request (see `firmware_changes`).
        self.firmware_changes(&driver);
        self.firmware_told(&driver);
        self.native_outbox_run();
        driver.try_borrow().map_or(true, |driver| driver.has_work())
    }

    /// Lets the device ops read the machine, once the host views exist.
    fn firmware_wire(
        &self,
        driver: &alloc::rc::Rc<core::cell::RefCell<super::firmware_driver::Driver>>,
    ) {
        let Some(hosts) = self.shared.native_instances.hosts() else {
            return;
        };
        if hosts.firmware_install().is_some() {
            return;
        }
        if let Ok(table) = driver.try_borrow() {
            let _ = hosts.wire_firmware_install(table.machine());
            let _ = hosts.wire_firmware_requests(alloc::rc::Rc::clone(driver));
        }
    }

    /// `event.firmware_changed` to the plugins whose requests are marked
    /// (see [`super::firmware_driver::Driver::peek_changed`]): one event per
    /// request however many steps passed, encoded from the state now. A
    /// plugin that cannot be called keeps its mark for the next pass.
    fn firmware_changes(
        &self,
        driver: &alloc::rc::Rc<core::cell::RefCell<super::firmware_driver::Driver>>,
    ) {
        for _ in 0..PER_PASS {
            let Some((owner, request)) = driver
                .try_borrow()
                .ok()
                .and_then(|driver| driver.peek_changed())
            else {
                break;
            };
            let state = driver.try_borrow().map_or(0, |driver| driver.state());
            let record = Record::EventFirmwareChanged(wire::EventFirmwareChanged {
                now_ms: self.now.millis(),
                request,
                state,
            });
            if self.firmware_tell(owner, wire::EVENT_FIRMWARE_CHANGED_TAG, &record) == Err(ERR_BUSY)
            {
                break;
            }
            if let Ok(mut driver) = driver.try_borrow_mut() {
                driver.clear_changed(owner, request);
            }
        }
    }

    /// Tells the plugins whose requests ended, a few a pass.
    fn firmware_told(
        &mut self,
        driver: &alloc::rc::Rc<core::cell::RefCell<super::firmware_driver::Driver>>,
    ) {
        for _ in 0..PER_PASS {
            self.native_outbox_run();
            if self.shared.native_instances.outbox.full() {
                break;
            }
            let Some(outcome) = driver
                .try_borrow()
                .ok()
                .and_then(|driver| driver.peek_ready())
            else {
                break;
            };
            let id = self
                .shared
                .native_instances
                .id(outcome.owner)
                .map_or("", |id| id.as_str());
            let record = Record::EventInstallDone(wire::EventInstallDone {
                now_ms: self.now.millis(),
                request: outcome.request,
                kind: outcome.kind,
                status: status_of(outcome),
                reason: u32::from(outcome.reason),
                flags: 0,
                id: if outcome.kind == super::firmware_install::KIND_FIRMWARE {
                    ""
                } else {
                    id
                },
            });
            if self.firmware_tell(outcome.owner, wire::EVENT_INSTALL_DONE_TAG, &record)
                == Err(ERR_BUSY)
            {
                // Its plugin cannot be called now: on a later pass.
                break;
            }
            if let Ok(mut driver) = driver.try_borrow_mut()
                && let Some(done) = driver.take_ready()
            {
                driver.delivered(done);
            }
        }
    }

    /// Calls `owner`'s event callback with `record`. `Ok` where it was called
    /// or there is nobody to call (stopped, not subscribed); `ERR_BUSY` where
    /// it cannot be called yet.
    fn firmware_tell(&self, owner: u64, tag: u32, record: &Record<'_>) -> Result<(), i32> {
        let instances = &self.shared.native_instances;
        if !instances.subscribed(owner, tag) {
            return Ok(());
        }
        let Ok(InstanceState::Running(lease)) = instances.state(owner) else {
            return Ok(());
        };
        let mut bytes = [0u8; RECORD_BYTES];
        let Ok(len) = codec::encode_batch(core::slice::from_ref(record), &mut bytes) else {
            debug_assert!(false, "an event {tag} record fits {RECORD_BYTES} bytes");
            return Ok(());
        };
        match self.native_event_call(owner, &lease, &bytes[..len]) {
            Err(ERR_BUSY) => Err(ERR_BUSY),
            // A failing callback is not told again.
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "firmware_events_tests.rs"]
mod tests;
