//! `event.widget_changed` (86, `docs/abi/ui.md` 5.2 "Updates") to the plugin
//! whose screen made the widget instance, after the instance's owner said it
//! shows something new with `command.widget_changed`.
//!
//! * The command is the owner's, for its own instances only: an unknown or
//!   dropped instance is `ERR_NOT_FOUND`, another plugin's `ERR_DENIED`
//!   (checked when the command is submitted, against the handle table).
//! * It is a hint. What the callback said is marked after it returned
//!   (`UiHandles::mark_changed`: one mark per instance however often it is
//!   said, allocation-free, room taken with the instance); this pump tells the
//!   maker between callbacks and never inside one. A mark is told once and
//!   never retried; an instance dropped meanwhile, or a maker that is stopped
//!   or did not subscribe to tag 86, is not told.
//! * At most [`PER_PASS`] events a pass, so one chatty widget does not starve
//!   the other pumps; the pump asks for another pass while marks wait. A batch
//!   a callback staged waits in the outbox and runs before the next callback.
//! * Memory: the marks are at most 32 `u32` on a large device and 16 on the X4
//!   (the widget cap), the record one 64-byte stack buffer.
use super::{
    codec,
    native_instances::InstanceState,
    registrations::{self as wire, Record},
};

/// Events told in one pass.
const PER_PASS: usize = 4;

/// The room of one record of event 86 (a batch of one is far under it).
const RECORD_BYTES: usize = 64;

impl crate::Kernel {
    /// Tells the makers of the widget instances said to have changed, a few a
    /// pass, outside any callback. `true` when marks wait (another pass).
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_widget_pump(&mut self) -> bool {
        // The taps first (`widget_inputs`), then the changes.
        let inputs = self.deliver_widget_inputs();
        self.widget_changed_pump() || inputs
    }

    /// The marks of changed instances, told to their makers. Its own frame
    /// (ppn5 frame limit), apart from the taps'.
    #[inline(never)]
    fn widget_changed_pump(&mut self) -> bool {
        let instances = &self.shared.native_instances;
        let Some(ui) = instances.ui_services() else {
            return false;
        };
        if !ui.widget_change_waits() {
            return false;
        }
        if instances.depth() > 0 {
            return true;
        }
        for _ in 0..PER_PASS {
            // What a callback staged runs before the next one is called.
            self.native_outbox_run();
            let instances = &self.shared.native_instances;
            if instances.outbox.full() {
                break;
            }
            let Some(ui) = instances.ui_services() else {
                break;
            };
            let Some((maker, instance)) = ui.take_widget_changed() else {
                break;
            };
            self.widget_changed_tell(maker, instance);
        }
        self.native_outbox_run();
        self.shared
            .native_instances
            .ui_services()
            .is_some_and(|ui| ui.widget_change_waits())
    }

    /// Tells `maker` that instance `instance` changed; nothing when it cannot
    /// be called or did not subscribe. Its own frame: the record and the
    /// call are not in the pump's (ppn5 frame limit).
    #[inline(never)]
    fn widget_changed_tell(&self, maker: u64, instance: u32) {
        let instances = &self.shared.native_instances;
        if !instances.subscribed(maker, wire::EVENT_WIDGET_CHANGED_TAG) {
            return;
        }
        let Ok(InstanceState::Running(lease)) = instances.state(maker) else {
            return;
        };
        let record = Record::EventWidgetChanged(wire::EventWidgetChanged {
            now_ms: self.now.millis(),
            instance,
        });
        let mut bytes = [0u8; RECORD_BYTES];
        let Ok(len) = codec::encode_batch(&[record], &mut bytes) else {
            debug_assert!(
                false,
                "an event.widget_changed record fits {RECORD_BYTES} bytes"
            );
            return;
        };
        // A failing callback is not told again.
        let _ = self.native_event_call(maker, &lease, &bytes[..len]);
    }
}

#[cfg(test)]
#[path = "widget_events_tests.rs"]
mod tests;
