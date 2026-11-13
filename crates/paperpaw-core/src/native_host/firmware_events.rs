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
/// How long a plugin that could not be called is left alone.
const RETRY_MS: u64 = 1_000;
/// Most grants of one plugin a removal forgets (a grant kind each).
const GRANTS_MAX: usize = 256;

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

/// The built-ins, and every plugin the kernel found (for the store's record).
struct Found<'a>(&'a dyn BuiltIns, &'a paperpaw_sdk::Plugins);
impl BuiltIns for Found<'_> {
    fn count(&self) -> usize {
        self.0.count()
    }
    fn index_of(&self, plugin_id: &str) -> Option<usize> {
        self.0.index_of(plugin_id)
    }
    fn found(&self) -> Option<&paperpaw_sdk::Plugins> {
        Some(self.1)
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
    /// What the device tells plugin jobs about itself, as
    /// [`KernelBuilder::plugin_host`](crate::KernelBuilder::plugin_host) does:
    /// for a shell that knows what it offers card plugins only once its
    /// services are wired, after the build. Before [`Kernel::start`]. Whether
    /// it took: plugin jobs share the firmware's queue, so without
    /// [`KernelBuilder::firmware_host`](crate::KernelBuilder::firmware_host)
    /// (or with the driver in use) nothing is set and they stay
    /// `ERR_UNSUPPORTED`.
    pub fn set_plugin_host(
        &mut self,
        loader: paperpaw_native::header5::Host,
        offered: paperpaw_native::metadata::Host<'static>,
        signers: &'static [(&'static str, [u8; 32])],
    ) -> bool {
        self.firmware.as_ref().is_some_and(|cell| {
            cell.set_plugin_host(super::plugin_job::PluginHost {
                loader,
                offered,
                signers,
            })
        })
    }

    /// The firmware driver, where one is made now (none at rest).
    pub(crate) fn firmware_driver(&self) -> Option<super::firmware_cell::DriverRef> {
        self.firmware.as_ref().and_then(|cell| cell.driver())
    }

    /// One step of the firmware table's job, then the outcomes that wait for
    /// their plugin. `true` while something is left (another pass).
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_firmware_pump(&mut self) -> bool {
        let Some(cell) = self.firmware.clone() else {
            return false;
        };
        self.firmware_wire(&cell);
        let now = self.now.millis();
        if self.shared.native_instances.depth() > 0 {
            return cell.resume_wanted()
                || cell.driver().is_some_and(|driver| {
                    driver
                        .try_borrow()
                        .map_or(true, |driver| driver.has_work(now))
                });
        }
        // A removal a cut left needs no driver: finished first, whether one
        // is made or not.
        if cell.take_resume() {
            self.firmware_resume();
        }
        let Some(driver) = cell.driver() else {
            return false;
        };
        self.native_outbox_run();
        self.feed_install_grants(&driver);
        {
            let Ok(mut driver) = driver.try_borrow_mut() else {
                return true;
            };
            let hosts = self.shared.native_instances.hosts();
            let plugins = &self.shared.plugins;
            match hosts.and_then(|hosts| hosts.device()) {
                Some(device) => {
                    let device = DeviceBuiltIns(&*device.0);
                    driver.pump(self.storage(), &Found(&device, plugins), now)
                }
                None => driver.pump(self.storage(), &Found(&NoBuiltIns, plugins), now),
            };
        }
        self.firmware_unload_pump(&driver);
        self.firmware_forget_pump(&driver);
        // Progress only marked the request (see `firmware_changes`).
        self.firmware_changes(&driver);
        self.firmware_told(&driver);
        let asked = self.firmware_confirm_pump(&driver);
        self.native_outbox_run();
        let more = asked
            || driver
                .try_borrow()
                .map_or(true, |driver| driver.has_work(now));
        // Nothing left to run, tell or keep: the driver goes until the next
        // command (`FirmwareCell::rest` keeps it while anything holds it).
        drop(driver);
        if !more {
            cell.rest();
        }
        more
    }

    /// Forgets what the host holds of a plugin removed with its data: its
    /// secrets (`core.secrets`, where wired) and its grants
    /// (`core.grant.<kind>.<id>`, decisions 124, 126 and 131), so a reinstall of
    /// the id does not regain what the user granted the old one. Safe to repeat.
    /// `false`: the secrets could not be forgotten (the journal stays, or the id
    /// goes on the list of `plugin_unforgotten`); the grants are gone either way.
    pub(crate) fn forget_removed(&mut self, plugin: &str) -> bool {
        let secrets = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.secrets())
            .map(|secrets| alloc::rc::Rc::clone(&secrets.host));
        self.forget_removed_by(plugin, || {
            secrets.is_none_or(|host| super::secrets_service::forget_owner(&*host, plugin).is_ok())
        })
    }

    /// The retry of an id on the list of `plugin_unforgotten`: as
    /// [`Self::forget_removed`], but a missing secrets host is **no** success
    /// (the id was listed because a host failed; its secrets are still there).
    pub(crate) fn forget_listed(&mut self, plugin: &str) -> bool {
        let secrets = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.secrets())
            .map(|secrets| alloc::rc::Rc::clone(&secrets.host));
        self.forget_removed_by(plugin, || {
            secrets.is_some_and(|host| super::secrets_service::forget_owner(&*host, plugin).is_ok())
        })
    }

    /// [`Self::forget_removed`] with the secrets' part given: the grants go
    /// whatever `forget_secrets` says (a grant left would let a later plugin of
    /// the id skip the install question).
    pub(crate) fn forget_removed_by(
        &mut self,
        plugin: &str,
        forget_secrets: impl FnOnce() -> bool,
    ) -> bool {
        let forgot = forget_secrets();
        self.forget_grants(plugin);
        forgot
    }

    /// The grants of `plugin`: only values that were set are reset (the file
    /// and the event follow [`Kernel::reset_setting`]).
    pub(crate) fn forget_grants(&mut self, plugin: &str) {
        // One at a time, so that no list is kept and none is left out. A
        // grant that stays set (a refused reset) ends the loop by the bound.
        for _ in 0..GRANTS_MAX {
            let next = self.shared.settings.specs().find_map(|spec| {
                let id = spec.id.as_str();
                let is_its = id
                    .strip_prefix("core.grant.")
                    .and_then(|rest| rest.split_once('.'))
                    .is_some_and(|(_, owner)| owner == plugin);
                (is_its && self.shared.settings.is_set(id)).then(|| spec.id.clone())
            });
            let Some(id) = next else {
                return;
            };
            self.reset_setting(&id);
        }
    }

    /// A removal that waits for the host to forget its plugin's state: done,
    /// the job goes on. Never from a callback.
    fn firmware_forget_pump(&mut self, driver: &super::firmware_cell::DriverRef) {
        let Some(id) = driver.try_borrow().ok().and_then(|d| d.peek_forget()) else {
            return;
        };
        let ok = self.forget_removed(id.as_str());
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.plugin_forgotten(ok);
        }
    }

    /// The first pass after a start that left a removal journal at its forget
    /// step: finished with the host's help, once.
    fn firmware_resume(&mut self) {
        let storage = self.shared.storage.clone();
        let done = super::plugin_swap::resume(storage.as_ref(), &mut |id| self.forget_removed(id));
        match done {
            super::plugin_swap::Replayed::Failed(why) => {
                self.warn_runtime(alloc::format!(
                    "plugins/.journal not finished (reason {why}): tried again at the next start"
                ));
            }
            super::plugin_swap::Replayed::Unforgotten { id } => {
                self.warn_runtime(alloc::format!(
                    "removal of {} finished without forgetting its secrets: the host failed {} starts; the id is kept in plugins/.unforgotten",
                    id.as_str(),
                    super::plugin_swap::FORGET_TRIES
                ));
                let text = self.shared.translator.format(
                    "core.plugin.unforgotten",
                    &[(
                        "id",
                        paperpaw_i18n::Arg::Str(&super::firmware_confirm::clean(id.as_str())),
                    )],
                );
                self.notify(text);
            }
            // Earlier removals whose secrets stayed: one try each at a start.
            _ => {
                super::plugin_unforgotten::retry(storage.as_ref(), &mut |id| {
                    self.forget_listed(id)
                });
            }
        }
    }

    /// A removal's first step: the instances of the plugin it removes are
    /// closed and the plugin leaves the list (REG op 6; event 70 is raised),
    /// then the job goes on. Never from a callback.
    fn firmware_unload_pump(&mut self, driver: &super::firmware_cell::DriverRef) {
        let Some(id) = driver.try_borrow().ok().and_then(|d| d.peek_unload()) else {
            return;
        };
        // An id that does not parse is none to close: the job goes on.
        if let Ok(id) = paperpaw_sdk::Id::parse(id.as_str()) {
            let _ = self.unload_plugin(&id);
            self.plugins_changed();
        }
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.plugin_unloaded();
        }
    }

    /// After a stop or a sender's stop: a plugin job that must finish (or runs
    /// on without its sender) is stepped on, bounded
    /// ([`super::firmware_driver::Driver::finish_stopped`]), at the end of
    /// this pass ([`Self::firmware_finish_due`]): not here, under the command
    /// or the callback that stopped it, where an install's signature check
    /// would run too deep for the X4's stack (#250).
    pub(crate) fn firmware_finish_stopped(&mut self, driver: &super::firmware_cell::DriverRef) {
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.want_finish();
        }
    }

    /// The bounded steps [`Self::firmware_finish_stopped`] asked for, taken
    /// once the pass's commands are applied and before its effects go to the
    /// shell (a USB drive takes the card only after them, a sleep only after
    /// them). At the pass's own level, outside every callback; not inlined,
    /// so its frames are not the pass's (#250).
    #[inline(never)]
    pub(crate) fn firmware_finish_due(&mut self) {
        if self.shared.native_instances.depth() > 0 {
            return;
        }
        // No driver made: nothing was asked to finish.
        let Some(driver) = self.firmware_driver() else {
            return;
        };
        if !driver.try_borrow().is_ok_and(|driver| driver.finish_due()) {
            return;
        }
        self.feed_install_grants(&driver);
        // Taken only with the driver in hand: a driver lent out now keeps
        // the steps asked for, and the next pass takes them.
        let Ok(mut driver) = driver.try_borrow_mut() else {
            return;
        };
        if !driver.take_finish() {
            return;
        }
        let hosts = self.shared.native_instances.hosts();
        match hosts.and_then(|hosts| hosts.device()) {
            Some(device) => driver.finish_stopped(self.storage(), &DeviceBuiltIns(&*device.0)),
            None => driver.finish_stopped(self.storage(), &NoBuiltIns),
        }
    }

    /// Lets the device ops and the plugins' commands reach the firmware, once
    /// the host views exist.
    fn firmware_wire(&self, cell: &alloc::rc::Rc<super::firmware_cell::FirmwareCell>) {
        let Some(hosts) = self.shared.native_instances.hosts() else {
            return;
        };
        if hosts.firmware_cell().is_none() {
            let _ = hosts.wire_firmware(alloc::rc::Rc::clone(cell));
        }
    }

    /// `event.firmware_changed` to the plugins whose requests are marked
    /// (see [`super::firmware_driver::Driver::peek_changed`]): one event per
    /// request however many steps passed, encoded from the state now. A
    /// plugin that cannot be called keeps its mark for the next pass.
    fn firmware_changes(&self, driver: &super::firmware_cell::DriverRef) {
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
                self.firmware_hold_back(driver);
                break;
            }
            if let Ok(mut driver) = driver.try_borrow_mut() {
                driver.clear_changed(owner, request);
                driver.released();
            }
        }
    }

    /// Tells the plugins whose requests ended, a few a pass.
    fn firmware_told(&mut self, driver: &super::firmware_cell::DriverRef) {
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
                flags: outcome.flags,
                id: if outcome.kind == super::firmware_install::KIND_FIRMWARE {
                    ""
                } else {
                    outcome.id.as_ref().map_or(id, |text| text.as_str())
                },
            });
            if self.firmware_tell(outcome.owner, wire::EVENT_INSTALL_DONE_TAG, &record)
                == Err(ERR_BUSY)
            {
                // Its plugin cannot be called now: on a later pass.
                self.firmware_hold_back(driver);
                break;
            }
            // Only the outcome that was told is marked: the list may have
            // changed during the callback.
            if let Ok(mut driver) = driver.try_borrow_mut() {
                if let Some(done) =
                    driver.take_ready_if(outcome.owner, outcome.request, outcome.kind)
                {
                    driver.delivered(done);
                }
                driver.released();
            }
        }
    }

    /// A plugin could not be called: no new try for [`RETRY_MS`], so the
    /// device may idle meanwhile (`Kernel::firmware_due` is the deadline).
    fn firmware_hold_back(&self, driver: &super::firmware_cell::DriverRef) {
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.hold_back(self.now.millis().saturating_add(RETRY_MS));
        }
    }

    /// When the pump looks again for what could not be told.
    pub(crate) fn firmware_due(&self) -> Option<u64> {
        self.firmware_driver()
            .and_then(|driver| driver.try_borrow().ok().and_then(|driver| driver.due()))
    }

    /// Calls `owner`'s event callback with `record`. `Ok` where it was called
    /// or there is nobody to call (stopped, not subscribed); `ERR_BUSY` where
    /// it cannot be called yet.
    fn firmware_tell(&self, owner: u64, tag: u32, record: &Record<'_>) -> Result<(), i32> {
        let instances = &self.shared.native_instances;
        if !instances.subscribed(owner, tag) {
            return Ok(());
        }
        let lease = match instances.state(owner) {
            Ok(InstanceState::Running(lease)) => lease,
            // Not started yet: it is started and told on a later pass.
            Ok(InstanceState::Lazy) => {
                instances.request_start(owner)?;
                return Err(ERR_BUSY);
            }
            // Stopped (or gone): nobody to tell.
            _ => return Ok(()),
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
