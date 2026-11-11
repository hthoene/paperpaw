//! The kernel's side of a plugin's own pictures (`command.image_prepare`,
//! `command.image_release`, `event.image_ready`; `document.md` section 5):
//! between callbacks it applies the admitted commands, decodes **one request
//! a turn**, and tells each answer to the plugin it belongs to.
//!
//! * The command stage admits the commands of a callback (`ImageAdmit`: root 1
//!   needs `uses.storage`, at most the plugin's room); after the callback
//!   returns they are applied ([`Kernel::native_images_apply`]). One that finds
//!   the table borrowed or full for now is kept (at most [`PENDING_MAX`] of them,
//!   room taken first) and applied again at the next pass; one that cannot be
//!   kept is dropped with a warning.
//! * The pump ([`Kernel::native_image_pump`], from the service pump, never
//!   inside a callback) first sweeps the cache folder once, a piece a pass (the
//!   files of images no plugin holds any more, left by a power cut), then each
//!   pass retries the kept commands, decodes the oldest request and tells up to
//!   [`READY_PER_PASS`] answers.
//! * An answer goes through the plugin's event registration for
//!   `event.image_ready` (a plugin that is not running, or did not subscribe, is
//!   not called and loses it, as its images end with it). A callback that fails
//!   ends the answer; only `ERR_BUSY` keeps it for the next pass. It is held
//!   here, outside the table's lock, so the plugin may use `image.info` in its
//!   callback.
//! * Failures that wait for their telling have a fixed table of eight places.
//!   When all are taken the oldest gives way, so with more than eight failing
//!   requests across plugins at once the plugin of an evicted one never hears
//!   of that request (a known gap, accepted: it needs a flood of failures at
//!   once; the plugin's own room still bounds what it may hold).
use super::{
    host_services::HostServices,
    image_service::{
        commands::{ImageCommand, ImageRoots},
        prepare::{Env as DecodeEnv, NO_MEMORY, Ready, SweepCursor, UNSUPPORTED},
    },
    native_instances::InstanceState,
    registrations::{self as wire},
};
use alloc::{format, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_NO_MEMORY, ERR_UNSUPPORTED};

/// Commands kept for a later pass: four requests a plugin may have waiting
/// and a release or two, for the few plugins that ask at once.
const PENDING_MAX: usize = 16;

/// Files of the cache folder removed in one pass of the boot sweep.
const SWEEP_PER_PASS: usize = 8;

/// Answers told in one pass.
const READY_PER_PASS: usize = 8;

/// The wait before a held-back command is applied again (the pump never asks
/// for an immediate pass for it: the device may sleep meanwhile).
const RETRY_MS: u64 = 2_000;

/// Times a held-back command is applied again before the plugin is told it
/// failed.
const TRIES_MAX: u8 = 3;

/// Failures waiting to be told to their plugin (a command that could not be
/// kept, or that ran out of tries): fixed, so telling a failure never needs
/// the memory that was missing.
const FAILED_MAX: usize = 8;

/// The room of an `event.image_ready` record (a header, 28 bytes and the
/// 64-byte brightness grid with its length).
const RECORD_BYTES: usize = 128;

/// What the pump keeps between passes.
#[derive(Default)]
pub(crate) struct ImagePump {
    /// Admitted commands the table could not take yet, oldest first, with the
    /// times each was tried again.
    pending: RefCell<Vec<(u64, ImageCommand, u8)>>,
    /// When the held-back commands are tried again.
    retry_at: Cell<Option<u64>>,
    /// Requests that failed, to be told: `(owner, cookie, status)`.
    failed: [Cell<Option<(u64, u32, u32)>>; FAILED_MAX],
    /// The failure to give way when all places are taken (the oldest).
    failed_next: Cell<usize>,
    /// The answer being told, kept where the table is not locked.
    held: RefCell<Option<Ready>>,
    /// The boot sweep of the cache folder is over.
    swept: Cell<bool>,
    cursor: Cell<SweepCursor>,
}

impl ImagePump {
    /// When the pump wants to run again for a held-back command.
    pub(crate) fn retry_at(&self) -> Option<u64> {
        self.retry_at.get()
    }
}

/// Where a plugin's root 0 and root 1 paths are on the card.
struct Roots<'a> {
    plugin: &'a str,
    hosts: &'a HostServices,
}

impl ImageRoots for Roots<'_> {
    fn resolve(&self, _owner: u64, root: u32, path: &str) -> Result<String, i32> {
        let first = path.split('/').next().unwrap_or_default();
        // The storage folder `first` is in, as the files service names it.
        let parent = if root == 0 {
            format!("{}/plugins/{}", paperpaw_sdk::data::DIR, self.plugin)
        } else {
            String::new()
        };
        let excluded = match root {
            // The host's own folder of received files.
            0 => first.eq_ignore_ascii_case(".received"),
            // The card's `plugins`, `.paperpaw` and `paperpaw.ini` (the files
            // service's rule).
            1 => super::owner_paths::protected_first(first),
            _ => return Err(ERR_UNSUPPORTED),
        } || self
            .hosts
            .files()
            .is_some_and(|files| files.0.excluded(root, &parent, first));
        if excluded {
            return Err(ERR_DENIED);
        }
        let prefix = if root == 0 {
            [paperpaw_sdk::data::DIR, "/plugins/", self.plugin, "/"]
        } else {
            ["", "", "", ""]
        };
        let length = prefix.iter().map(|part| part.len()).sum::<usize>() + path.len();
        let mut full = String::new();
        full.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
        for part in prefix {
            full.push_str(part);
        }
        full.push_str(path);
        Ok(full)
    }
}

/// The decoder's view of the device.
fn decode_env(device: &paperpaw_platform::DeviceInfo) -> DecodeEnv {
    let screen = u32::from(device.display.width.max(device.display.height));
    let colour = device.display.color.is_some();
    if super::ui_store::is_x4(device.memory) {
        DecodeEnv::x4(screen, colour)
    } else {
        DecodeEnv {
            screen,
            colour,
            max_side: super::image_service::MAX_SIDE,
            png_row_max: usize::MAX,
            turn_band: 1 << 20,
            turn_reads: usize::MAX,
        }
    }
}

/// Whether a refusal is for now: the command is kept and tried again.
fn for_now(status: i32) -> bool {
    matches!(status, ERR_BUSY | ERR_FULL | ERR_NO_MEMORY)
}

impl crate::Kernel {
    /// Applies the image commands a callback of `owner` staged, in order.
    /// What the table cannot take now is kept for the next pass (and what
    /// follows it for the same plugin, to keep their order).
    pub(crate) fn native_images_apply(&mut self, owner: u64, commands: Vec<ImageCommand>) {
        let mut behind = self
            .shared
            .native_instances
            .image_pump
            .pending
            .borrow()
            .iter()
            .any(|(who, ..)| *who == owner);
        for command in commands {
            if !behind {
                match self.image_apply_one(owner, &command) {
                    Some(status) if for_now(status) => behind = true,
                    // A lasting refusal: the submit said yes, the plugin is
                    // told it failed.
                    Some(_) => {
                        if !self.image_fail(owner, &command, UNSUPPORTED) {
                            self.warn_runtime("a plugin's picture failure replaced an older one");
                        }
                        continue;
                    }
                    None => continue,
                }
            }
            self.image_keep(owner, command);
        }
    }

    /// `None` when done or dropped, the status when it is for now.
    fn image_apply_one(&self, owner: u64, command: &ImageCommand) -> Option<i32> {
        let instances = &self.shared.native_instances;
        let hosts = instances.hosts()?;
        // A plugin that is gone has no pictures to ask for.
        let plugin = instances.id(owner)?;
        let roots = Roots {
            plugin: plugin.as_str(),
            hosts,
        };
        hosts.images().apply(owner, command, &roots).err()
    }

    fn image_keep(&mut self, owner: u64, command: ImageCommand) {
        let pump = &self.shared.native_instances.image_pump;
        let kept = {
            let mut pending = pump.pending.borrow_mut();
            if pending.len() < PENDING_MAX && pending.try_reserve(1).is_ok() {
                pending.push((owner, command, 0));
                true
            } else {
                drop(pending);
                // Not kept: its plugin is told it failed, in the room the
                // failures have of their own.
                self.image_fail(owner, &command, NO_MEMORY);
                false
            }
        };
        if kept {
            // Tried again later, not at once.
            let pump = &self.shared.native_instances.image_pump;
            if pump.retry_at.get().is_none() {
                pump.retry_at.set(Some(self.now.millis() + RETRY_MS));
            }
        } else {
            self.warn_runtime("a plugin's picture command could not be kept for later; it failed");
        }
    }

    /// A request that failed is told to its plugin as `event.image_ready`
    /// with `status` (5 not enough memory now; 1 a request that can never be
    /// done). A release has nothing to tell. With every place taken the oldest
    /// failure gives way (`false`: the caller says so): the plugins' requests
    /// are counted against their room (`image_held`), so this takes more
    /// failing plugins than the places there are.
    fn image_fail(&self, owner: u64, command: &ImageCommand, status: u32) -> bool {
        let ImageCommand::Prepare { cookie, .. } = command else {
            return true;
        };
        let pump = &self.shared.native_instances.image_pump;
        let entry = Some((owner, *cookie, status));
        if let Some(slot) = pump.failed.iter().find(|slot| slot.get().is_none()) {
            slot.set(entry);
            return true;
        }
        let at = pump.failed_next.get() % FAILED_MAX;
        pump.failed[at].set(entry);
        pump.failed_next.set(at + 1);
        false
    }

    /// The requests of `owner` that wait here (kept or failed, not yet
    /// told): they count against its room like the table's own.
    pub(crate) fn image_held(&self, owner: u64) -> usize {
        let pump = &self.shared.native_instances.image_pump;
        let kept = pump
            .pending
            .borrow()
            .iter()
            .filter(|(who, command, _)| {
                *who == owner && matches!(command, ImageCommand::Prepare { .. })
            })
            .count();
        let failed = pump
            .failed
            .iter()
            .filter(|slot| slot.get().is_some_and(|(who, ..)| who == owner))
            .count();
        kept + failed
    }

    /// One pass of the picture pump (see the module). `true` when work
    /// remains for another pass.
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_image_pump(&mut self) -> bool {
        if self.shared.native_instances.depth() == 0 {
            self.native_outbox_run();
        }
        let instances = &self.shared.native_instances;
        // Never inside a callback: what is asked during one comes after.
        if instances.depth() > 0 {
            return true;
        }
        let Some(hosts) = instances.hosts() else {
            return false;
        };
        let table = hosts.images();
        let pump = &instances.image_pump;
        let storage = self.shared.storage.clone();
        // The cache folder is swept before the first request is decoded.
        if !pump.swept.get() {
            let mut cursor = pump.cursor.get();
            match table.sweep_cache(storage.as_ref(), SWEEP_PER_PASS, &mut cursor) {
                Ok(true) | Err(ERR_BUSY) => {
                    pump.cursor.set(cursor);
                    return true;
                }
                // Done, or the card cannot be listed: what is left goes at a
                // later start.
                Ok(false) | Err(_) => pump.swept.set(true),
            }
        }
        self.image_retry();
        let env = decode_env(self.shared.env(self.now).device());
        let mut more = self.shared.native_instances.hosts().is_some_and(|hosts| {
            // One request a turn, decoded to the end.
            hosts.images().prepare_step(storage.as_ref(), env).is_err()
        });
        more |= self.image_tell_ready();
        let instances = &self.shared.native_instances;
        // A held-back command waits for its own time (`retry_at`), it does not
        // ask for another pass.
        more || instances
            .hosts()
            .is_some_and(|hosts| hosts.images().prepare_pending())
            || instances
                .image_pump
                .failed
                .iter()
                .any(|slot| slot.get().is_some())
    }

    /// Kept commands, oldest first, once more, when their time came. One that
    /// still finds no room after [`TRIES_MAX`] tries fails.
    fn image_retry(&mut self) {
        let now = self.now.millis();
        let pump = &self.shared.native_instances.image_pump;
        if pump.retry_at.get().is_none_or(|at| now < at) {
            return;
        }
        pump.retry_at.set(None);
        let (mut at, mut lost) = (0, false);
        // No borrow is held across the call.
        while let Some((owner, command, tries)) = {
            let mut list = pump.pending.borrow_mut();
            (at < list.len()).then(|| list.remove(at))
        } {
            match self.image_apply_one(owner, &command) {
                Some(status) if for_now(status) && tries + 1 < TRIES_MAX => {
                    // Its place was just left: no allocation.
                    pump.pending
                        .borrow_mut()
                        .insert(at, (owner, command, tries + 1));
                    at += 1;
                }
                Some(status) if for_now(status) => {
                    lost |= !self.image_fail(owner, &command, NO_MEMORY);
                }
                // A lasting refusal: the submit said yes, so the plugin is told.
                Some(_) => lost |= !self.image_fail(owner, &command, UNSUPPORTED),
                None => {}
            }
        }
        if !pump.pending.borrow().is_empty() {
            pump.retry_at.set(Some(now + RETRY_MS));
        }
        if lost {
            self.warn_runtime("a plugin's picture failure replaced an older one");
        }
    }

    /// Tells up to [`READY_PER_PASS`] answers; `true` when one is left.
    fn image_tell_ready(&mut self) -> bool {
        for _ in 0..READY_PER_PASS {
            let instances = &self.shared.native_instances;
            let Some(hosts) = instances.hosts() else {
                return false;
            };
            let pump = &instances.image_pump;
            if pump.held.borrow().is_none()
                && let Some(slot) = pump.failed.iter().find(|slot| slot.get().is_some())
                && let Some((owner, cookie, status)) = slot.take()
            {
                *pump.held.borrow_mut() = Some(Ready {
                    owner,
                    cookie,
                    status,
                    image: 0,
                    width: 0,
                    height: 0,
                    luma: None,
                });
            }
            if pump.held.borrow().is_none() {
                // Copied out and left at once: the plugin runs with the table
                // free.
                let taken = hosts.images().take_ready(&mut |ready| {
                    *pump.held.borrow_mut() = Some(ready.clone());
                    Ok(())
                });
                match taken {
                    Ok(true) => {}
                    Ok(false) => return false,
                    Err(_) => return true,
                }
            }
            let Some(ready) = pump.held.borrow().clone() else {
                return false;
            };
            let outcome = self.image_deliver(&ready);
            // What the callback staged runs before the next one is called.
            self.native_outbox_run();
            if outcome == Some(ERR_BUSY) {
                return true;
            }
            self.shared
                .native_instances
                .image_pump
                .held
                .borrow_mut()
                .take();
        }
        true
    }

    /// `Some(status)` of the plugin's callback when it failed; `None` when it
    /// took the answer or there was no one to tell.
    fn image_deliver(&self, ready: &Ready) -> Option<i32> {
        let instances = &self.shared.native_instances;
        let owner = ready.owner;
        if !instances.subscribed(owner, wire::EVENT_IMAGE_READY_TAG) {
            return None;
        }
        let Ok(InstanceState::Running(lease)) = instances.state(owner) else {
            return None;
        };
        let mut record = [0u8; RECORD_BYTES];
        let len = wire::records_encode(&[ready.record(self.now.millis())], &mut record).ok()?;
        self.native_event_call(owner, &lease, &record[..len]).err()
    }
}

#[cfg(test)]
#[path = "image_pump_tests.rs"]
mod tests;
