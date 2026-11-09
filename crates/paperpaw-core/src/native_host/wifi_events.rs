//! `event.wifi_done` (93) to the plugin whose join or host request ended
//! (`docs/abi/device.md` 4): one event per request, called at most once.
//!
//! * **Never inside a callback.** The pump does nothing while a native
//!   callback is on the stack, and a batch a callback staged runs before the
//!   next callback is called.
//! * **Told to the sender only.** The record waits in the request table, in the
//!   room reserved when the request was accepted, until its plugin can be
//!   called. The entry is released once the callback was called, whatever it
//!   answered (only a busy answer is tried again, so a failing callback does
//!   not get the event twice), and at once for a plugin that stopped or did
//!   not subscribe: four entries are held device-wide, so none may wait for
//!   nobody.
//! * **One plugin does not hold up another.** A plugin that cannot be called
//!   now (its callback is busy, or it is not started) is not tried again for
//!   [`RETRY_MS`], which every waiting request shares: the main loop wakes at
//!   `native_wifi_due` and the device may idle until then. The next ended
//!   request is tried in the same pass.
//! * **A plugin that never starts does not keep its entry.** The host asks
//!   for its start at most [`MAX_STARTS`] times; then the entry is released
//!   and the event is lost.
use super::{
    native_instances::InstanceState,
    radio_requests::{MAX_REQUESTS, RadioRequests},
    registrations as wire,
};
use crate::Kernel;
use paperpaw_native::host_glue::ERR_BUSY;

/// The room of one `wifi_done` record (a batch of one).
const RECORD_BYTES: usize = 128;
/// How long a plugin that could not be called is left alone.
const RETRY_MS: u64 = 1_000;
/// How often a plugin that is not started is asked to start for its event.
const MAX_STARTS: u8 = 3;

impl Kernel {
    /// Tells the plugins whose Wi-Fi requests ended. `true` while something is
    /// left to tell right now (another pass); a plugin that could not be
    /// called is tried again at [`Self::native_wifi_due`].
    pub(crate) fn native_wifi_pump(&mut self) -> bool {
        let table = alloc::rc::Rc::clone(&self.radio_requests);
        let now = self.now.millis();
        match table.try_borrow() {
            Ok(table) if !table.has_outcomes() => return false,
            Ok(table) if table.held(now) => return false,
            Err(_) => return true,
            Ok(_) => {}
        }
        if self.shared.native_instances.depth() > 0 {
            return true;
        }
        // What a callback staged runs before the next one is called.
        self.native_outbox_run();
        let mut nth = 0;
        for _ in 0..=MAX_REQUESTS {
            if self.shared.native_instances.outbox.full() {
                break;
            }
            let told = self.wifi_tell(&table, nth, now);
            match told {
                Some(true) => self.native_outbox_run(),
                Some(false) => nth += 1,
                None => break,
            }
        }
        self.native_outbox_run();
        table
            .try_borrow()
            .map_or(true, |table| table.has_outcomes() && !table.held(now))
    }

    /// When a plugin that could not be called is tried again.
    pub(crate) fn native_wifi_due(&self) -> Option<u64> {
        self.radio_requests
            .try_borrow()
            .ok()
            .and_then(|table| table.due())
    }

    /// Tells the `nth` ended request to its sender. `Some(true)` when it was
    /// released (told, or nobody to tell), `Some(false)` when its plugin cannot
    /// be called now, `None` when there is no such request.
    fn wifi_tell(
        &self,
        table: &core::cell::RefCell<RadioRequests>,
        nth: usize,
        now: u64,
    ) -> Option<bool> {
        let instances = &self.shared.native_instances;
        let mut bytes = [0u8; RECORD_BYTES];
        let (key, request, len) = {
            let table = table.try_borrow().ok()?;
            let (owner, delivery) = table.nth_outcome(nth)?;
            (
                instances.find(owner),
                delivery.request,
                delivery.encode(now, &mut bytes).ok(),
            )
        };
        if let (Some(key), Some(len)) = (key, len) {
            match instances.state(key) {
                Ok(InstanceState::Running(lease)) => {
                    // Not subscribed: nobody to tell.
                    if instances.subscribed(key, wire::EVENT_WIFI_DONE_TAG)
                        && self.native_event_call(key, &lease, &bytes[..len]) == Err(ERR_BUSY)
                    {
                        self.wifi_hold_back(table, now);
                        return Some(false);
                    }
                }
                // Not started: ask for its start and tell it on a later pass, up
                // to `MAX_STARTS` times (else its entry is released below).
                Ok(InstanceState::Lazy) => {
                    let starts = table
                        .try_borrow_mut()
                        .map_or(u8::MAX, |mut table| table.note_start(nth, request));
                    if starts <= MAX_STARTS && instances.request_start(key).is_ok() {
                        self.wifi_hold_back(table, now);
                        return Some(false);
                    }
                }
                // Stopped: nobody to tell.
                _ => {}
            }
        }
        // Only the request that was told is released: the table may have
        // changed during the callback.
        let owner = key.and_then(|key| instances.id(key));
        if let (Ok(mut table), Some(owner)) = (table.try_borrow_mut(), owner) {
            table.delivered(owner.as_str(), request);
        } else if let Ok(mut table) = table.try_borrow_mut() {
            // The sender is gone altogether: its entry is released as stopped.
            table.release_ended_at(nth, request);
        }
        Some(true)
    }

    fn wifi_hold_back(&self, table: &core::cell::RefCell<RadioRequests>, now: u64) {
        if let Ok(mut table) = table.try_borrow_mut() {
            table.hold_back(now.saturating_add(RETRY_MS));
        }
    }
}

#[cfg(test)]
#[path = "wifi_events_tests.rs"]
mod tests;
