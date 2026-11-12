//! `event.stack_changed` (81, `docs/abi/ui.md` 5.1): a hint to the plugins
//! that subscribed to it, after the screen stack changed (a screen opened,
//! closed or came to the top).
//!
//! * The stack counts its changes (`StackRows::changes`, bumped when an update
//!   leaves a different list); this pump tells each subscribed, running
//!   plugin whose last told count is not the current one. Coalescing is
//!   free: any number of changes between two passes are one hint, and the
//!   record (the time only) is built at delivery. The plugin reads the stack
//!   with `core.ui` op 1.
//! * A plugin that starts subscribing is not told of what happened before it
//!   looked: its first sight of the stack records the count and says
//!   nothing.
//! * The count told is set before the callback is called, so a change the
//!   callback causes is told again by the next pass, never inside it.
//! * Nothing is told inside a callback, and at most [`PER_PASS`] plugins a
//!   pass; the pump asks for another pass while one waits.
//! * Memory: one `(u64, u32)` per subscribed plugin, reserved fallibly (a
//!   plugin the table has no room for is not told), the record one 32-byte
//!   stack buffer. Entries of plugins that stopped or no longer subscribe
//!   are removed on each pass.
use super::{
    codec,
    native_instances::InstanceState,
    registrations::{self as wire, Record},
};

/// Plugins told in one pass.
const PER_PASS: usize = 4;

/// The room of one record of event 81.
const RECORD_BYTES: usize = 32;

impl crate::Kernel {
    /// Tells the plugins that subscribed to `event.stack_changed` that the
    /// stack changed, a few a pass, outside any callback. `true` when a
    /// plugin still waits (another pass).
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_stack_pump(&mut self) -> bool {
        if self.shared.native_instances.depth() > 0 {
            return self.stack_changed_waits();
        }
        for _ in 0..PER_PASS {
            // What a callback staged runs before the next one is called.
            self.native_outbox_run();
            if self.shared.native_instances.outbox.full() {
                break;
            }
            let Some(owner) = self.stack_changed_next() else {
                break;
            };
            self.stack_changed_tell(owner);
        }
        self.native_outbox_run();
        self.stack_changed_waits()
    }

    /// The next subscribed, running plugin to tell (its count is taken as told),
    /// after the entries of plugins that left were removed and the new ones
    /// recorded.
    fn stack_changed_next(&self) -> Option<u64> {
        let instances = &self.shared.native_instances;
        let rows = &self.shared.stack_rows;
        let now = rows.changes();
        if let Ok(mut told) = rows.told.try_borrow_mut() {
            told.retain(|(owner, _)| instances.subscribed(*owner, wire::EVENT_STACK_CHANGED_TAG));
        } else {
            return None;
        }
        let mut index = 0;
        let mut next = None;
        while let Some((owner, _)) = instances.running_nth(index) {
            index += 1;
            if !instances.subscribed(owner, wire::EVENT_STACK_CHANGED_TAG) {
                continue;
            }
            // Looked at in a short borrow: no borrow of the table is held
            // while a parked plugin is made below (#248).
            let due = {
                let Ok(mut told) = rows.told.try_borrow_mut() else {
                    return next;
                };
                match told.iter().find(|(known, _)| *known == owner) {
                    // First sight: the count is recorded, nothing is said (a
                    // parked plugin is not made for it).
                    None => {
                        if told.try_reserve(1).is_ok() {
                            told.push((owner, now));
                        }
                        false
                    }
                    Some((_, count)) => *count != now && next.is_none(),
                }
            };
            // A parked plugin is made first; one that cannot be made now
            // keeps its count and is told on a later pass.
            debug_assert!(
                rows.told.try_borrow_mut().is_ok(),
                "no borrow across an unpark"
            );
            if due && self.native_ready(owner) {
                if let Ok(mut told) = rows.told.try_borrow_mut()
                    && let Some((_, count)) = told.iter_mut().find(|(known, _)| *known == owner)
                {
                    *count = now;
                }
                next = Some(owner);
            }
        }
        next
    }

    /// Whether a subscribed plugin has not been told the current count.
    fn stack_changed_waits(&self) -> bool {
        let rows = &self.shared.stack_rows;
        let now = rows.changes();
        rows.told.try_borrow().is_ok_and(|told| {
            told.iter().any(|(owner, count)| {
                *count != now
                    && self
                        .shared
                        .native_instances
                        .subscribed(*owner, wire::EVENT_STACK_CHANGED_TAG)
                    && !self.native_parked(*owner)
            })
        })
    }

    /// Tells `owner`; nothing when it cannot be called.
    fn stack_changed_tell(&self, owner: u64) {
        let instances = &self.shared.native_instances;
        let Ok(InstanceState::Running(lease)) = instances.state(owner) else {
            return;
        };
        let record = Record::EventStackChanged(wire::EventStackChanged {
            now_ms: self.now.millis(),
        });
        let mut bytes = [0u8; RECORD_BYTES];
        let Ok(len) = codec::encode_batch(&[record], &mut bytes) else {
            debug_assert!(
                false,
                "an event.stack_changed record fits {RECORD_BYTES} bytes"
            );
            return;
        };
        // A failing callback is not told again.
        let _ = self.native_event_call(owner, &lease, &bytes[..len]);
    }
}

#[cfg(test)]
#[path = "stack_events_tests.rs"]
mod tests;
