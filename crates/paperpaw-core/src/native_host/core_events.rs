//! The kernel's own events (`core.tick` 3, `started` 32, `resume` 33,
//! `suspend` 34, `stopping` 35, `battery_measured` 36 and `page_turned` 37) to
//! the native plugins that subscribed to them (`core-events.md` sections 3.2
//! and 3.3). They are raised in [`Kernel::emit_to`](crate::Kernel), the one
//! place every event of the kernel passes, and queued in the same per-plugin
//! queue as the setting events ([`SettingEvents`](super::setting_events)), so
//! a plugin hears everything in one order.
//!
//! * **Queued, never called from `emit_to`.** The event is queued; the pump
//!   ([`Kernel::native_setting_pump`]) tells it between callbacks and does
//!   nothing while any native callback (any plugin's, nested included) is on
//!   the stack. The one synchronous part is the drain right after `suspend`
//!   or `stopping` is raised: from the kernel's top level, before sleep or
//!   teardown goes on, it tells what waits for the plugin, in order (edges
//!   raised earlier first). A callback that refuses is logged and dropped,
//!   never retried, and the sleep goes on.
//! * **Order.** An event never overtakes one already queued for the plugin.
//! * **Memory.** The edges (`started`, `battery_measured`, `page_turned`) are
//!   held encoded, within the store's limits (16 edges and 4,096 bytes a
//!   plugin, 8,192 bytes device-wide). One that does not fit is dropped,
//!   logged and never retried. Tick and resume keep one pending bit each (a
//!   second before the first was told is one); suspend and stopping keep one
//!   too, so a full store never loses them (`events::bypasses_queue`). The
//!   record of a bit is built when it is told.
//! * **Only running plugins** hear them (lazy start: registering starts
//!   nothing), and only those that subscribed. The caller names who may hear
//!   (the plugin an event is for alone, or the screens of a guard).
use super::{
    event_pending::{CORE_TAGS, Pending},
    events,
    native_instances::NativeInstances,
    registrations as wire,
};
use crate::Kernel;
use alloc::{format, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_sdk::{Id, Params, builtin::events as builtin};

/// The most events one plugin's queue can hold (16 edges, 10 bits): a drain
/// of one plugin ends within it.
const QUEUE_MAX: usize = 26;

/// The lifecycle events that are edges, held encoded.
const EDGE_TAGS: [u32; 3] = [
    wire::EVENT_STARTED_TAG,
    wire::EVENT_BATTERY_MEASURED_TAG,
    wire::EVENT_PAGE_TURNED_TAG,
];

/// The wire tag of the kernel event `event`, `None` for any other event.
fn tag_of(event: &Id) -> Option<u32> {
    Some(if *event == builtin::STARTED {
        wire::EVENT_STARTED_TAG
    } else if *event == builtin::RESUME {
        wire::EVENT_RESUME_TAG
    } else if *event == builtin::SUSPEND {
        wire::EVENT_SUSPEND_TAG
    } else if *event == builtin::STOPPING {
        wire::EVENT_STOPPING_TAG
    } else if *event == builtin::BATTERY_MEASURED {
        wire::EVENT_BATTERY_MEASURED_TAG
    } else if *event == builtin::PAGE_TURNED {
        wire::EVENT_PAGE_TURNED_TAG
    } else if *event == super::native_tick::TICK {
        wire::EVENT_TICK_TAG
    } else {
        return None;
    })
}

/// Whether `owner` subscribed to any of these events.
pub(super) fn listens(instances: &NativeInstances, owner: u64) -> bool {
    CORE_TAGS
        .iter()
        .chain(&EDGE_TAGS)
        .any(|&tag| instances.subscribed(owner, tag))
}

/// The record of the bit event `tag` into `bytes`, with the clock `now_ms`
/// (one record of at most 12 bytes).
pub(super) fn scalar(tag: u32, now_ms: u64, bytes: &mut Vec<u8>) -> Result<usize, i32> {
    bytes.try_reserve_exact(16).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(16, 0);
    match tag {
        wire::EVENT_TICK_TAG => wire::tick_encode(now_ms, bytes),
        wire::EVENT_RESUME_TAG => events::encode_resume(now_ms, bytes),
        wire::EVENT_SUSPEND_TAG => events::encode_suspend(now_ms, bytes),
        wire::EVENT_STOPPING_TAG => events::encode_stopping(now_ms, bytes),
        _ => Err(ERR_INVALID),
    }
}

/// Holds one edge in `queue`. Its own function: the page turn's record is the
/// largest (under 1.1 KB) and must not widen the frame of the caller
/// (no frame over 2,048 bytes on the X4).
#[inline(never)]
fn hold_edge(queue: &mut Pending, tag: u32, now_ms: u64, params: &Params) -> Result<(), i32> {
    match tag {
        wire::EVENT_STARTED_TAG => {
            let record = wire::Record::EventStarted(wire::EventStarted { now_ms });
            queue.push_record(tag, &record)
        }
        wire::EVENT_BATTERY_MEASURED_TAG => {
            let record = wire::Record::EventBatteryMeasured(wire::EventBatteryMeasured { now_ms });
            queue.push_record(tag, &record)
        }
        _ => {
            let turn = super::flat::page::turn(now_ms, params).ok_or(ERR_INVALID)?;
            let record = wire::Record::EventPageTurned(events::page_turned_record(&turn)?);
            queue.push_record(tag, &record)
        }
    }
}

impl Kernel {
    /// Queues the kernel event `event` for the running native plugins that
    /// subscribed to it and `hears` allows. Suspend and stopping are told
    /// at once (see the module); the pump tells the others. Any other event
    /// is left alone.
    pub(crate) fn native_core_event(
        &mut self,
        event: &Id,
        params: &Params,
        hears: &dyn Fn(&str) -> bool,
    ) {
        let Some(tag) = tag_of(event) else {
            return;
        };
        let now = self.now.millis();
        let mut told = Vec::new();
        let mut n = 0;
        let mut lost = Vec::new();
        {
            let instances = &self.shared.native_instances;
            while let Some((owner, _)) = instances.running_nth(n) {
                n += 1;
                let Some(id) = instances.id(owner) else {
                    continue;
                };
                if !instances.subscribed(owner, tag) || !hears(id.as_str()) {
                    continue;
                }
                let held = instances.setting_events.with_queue(owner, |queue| {
                    if CORE_TAGS.contains(&tag) {
                        queue.core(tag)
                    } else {
                        hold_edge(queue, tag, now, params)
                    }
                });
                match held {
                    Ok(Ok(())) => {}
                    Ok(Err(status)) | Err(status) => {
                        lost.push((id.clone(), status));
                        continue;
                    }
                }
                // One entry a plugin: a failed reserve only delays the pump.
                if told.try_reserve(1).is_ok() {
                    told.push(owner);
                }
            }
        }
        for (plugin, status) in lost {
            self.warn_runtime(format!(
                "event {tag} dropped for plugin `{plugin}` (status {status}); not retried"
            ));
        }
        // Only the sleep and the stop are told here, from the kernel's top
        // level, before they go on; the pump tells every other event.
        let sleeps = matches!(tag, wire::EVENT_SUSPEND_TAG | wire::EVENT_STOPPING_TAG);
        if sleeps && self.shared.native_instances.depth() == 0 {
            for owner in told {
                self.native_core_drain(owner);
            }
        }
    }

    /// Tells `owner` what waits for it, oldest first, running what each
    /// callback staged before the next one is called. Stops where the outbox
    /// stays full; the pump tells the rest.
    fn native_core_drain(&mut self, owner: u64) {
        for _ in 0..QUEUE_MAX {
            self.native_outbox_run();
            if self.shared.native_instances.outbox.full() {
                return;
            }
            let events = &self.shared.native_instances.setting_events;
            if !events.has_waiting(owner) || !self.setting_tell(owner) {
                break;
            }
        }
        self.native_outbox_run();
    }
}

#[cfg(test)]
#[path = "core_events_tests.rs"]
mod tests;
