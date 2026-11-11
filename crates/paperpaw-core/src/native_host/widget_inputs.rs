//! The taps and long presses a showing screen forwarded to a widget instance
//! (`core.ui` op 6, `docs/abi/ui.md` 5.2), delivered to the instance's maker
//! at its widget `input` callback.
//!
//! * The pump drains what op 6 queued ([`UiServices::next_widget_input`]),
//!   between callbacks and never inside one, a few a pass; it asks for
//!   another pass while taps wait. The first pass marks op 6 as served
//!   ([`UiServices::widget_input_drained`]): until then a tap nothing would
//!   deliver is refused.
//! * The guard rule of ui.md 4.3 applies again at the time of delivery
//!   ([`UiServices::widget_input_allowed`]): a guard may have come up since
//!   the tap was queued, and the tap then goes nowhere. An instance dropped
//!   meanwhile, or whose owner stopped or cannot be called, is never
//!   delivered to either.
//! * The callback gets its command stage (`screens-input.md` 2.1); what it
//!   staged waits in the outbox and runs before the next callback. A tap is
//!   tried once whatever came of it: the callback returned, failed or
//!   trapped.
//! * Memory: the record is one 64-byte stack buffer; no allocation.
use super::ui_guard::UiStackView;
use super::{
    native_instances::InstanceState,
    stack_rows::StackRows,
    ui_guard::{guard_owner, topmost_guard},
    ui_handles::GuardState,
};
use paperpaw_native::host_glue::ERR_BUSY;

/// The room of one `input.widget` record.
const RECORD_BYTES: usize = 64;

/// Taps delivered in one pass.
const PER_PASS: usize = 4;

/// The stack's guard as the kernel reads it between callbacks. A stack that
/// is not current fails closed: guarded by no one's plugin, everything below.
struct RowsGuard<'a>(&'a StackRows);

impl GuardState for RowsGuard<'_> {
    fn guard_plugin(&self) -> Option<u64> {
        guard_owner(self.0)
    }

    fn on_stack(&self, screen_instance: u32) -> bool {
        let view: &dyn UiStackView = self.0;
        (0..view.len()).any(|i| view.row(i).is_some_and(|r| r.instance == screen_instance))
    }

    fn below_guard(&self, screen_instance: u32) -> bool {
        let view: &dyn UiStackView = self.0;
        let at =
            (0..view.len()).find(|&i| view.row(i).is_some_and(|r| r.instance == screen_instance));
        match (topmost_guard(view), at) {
            (Some(guard), Some(at)) => at <= guard,
            // A screen the stack does not hold, under a guard: fail closed.
            (Some(_), None) => true,
            (None, _) => false,
        }
    }
}

impl crate::Kernel {
    /// Delivers the taps waiting for their widgets' makers, a few a pass,
    /// outside any callback. `true` while taps wait (another pass).
    pub(crate) fn deliver_widget_inputs(&mut self) -> bool {
        let Some(ui) = self.shared.native_instances.ui_services() else {
            return false;
        };
        // The pump runs: op 6 may queue from now on.
        let _ = ui.widget_input_drained();
        if !matches!(ui.next_widget_input_owner(), Ok(Some(_))) {
            return false;
        }
        if self.shared.native_instances.depth() > 0 {
            return true;
        }
        for _ in 0..PER_PASS {
            // What a callback staged runs before the next one is called.
            self.native_outbox_run();
            if self.shared.native_instances.outbox.full() {
                break;
            }
            if !self.deliver_one_widget_input() {
                break;
            }
        }
        self.native_outbox_run();
        self.shared
            .native_instances
            .ui_services()
            .is_some_and(|ui| matches!(ui.next_widget_input_owner(), Ok(Some(_))))
    }

    /// The first waiting tap, delivered or discarded. `false` when none
    /// waits or the table is in use (the next pass tries again).
    fn deliver_one_widget_input(&self) -> bool {
        let instances = &self.shared.native_instances;
        let Some(ui) = instances.ui_services() else {
            return false;
        };
        let Ok(Some(owner)) = ui.next_widget_input_owner() else {
            return false;
        };
        let mut record = [0u8; RECORD_BYTES];
        let (input, len) = match ui.next_widget_input(owner, self.now.millis(), &mut record) {
            Ok(Some(found)) => found,
            other => {
                // A record that does not fit its buffer is a bug; a table in
                // use leaves the tap for the next pass.
                debug_assert!(
                    other.is_ok() || other == Err(ERR_BUSY),
                    "an input.widget record fits {RECORD_BYTES} bytes: {other:?}"
                );
                return false;
            }
        };
        let allowed = ui.widget_input_allowed(&input, &RowsGuard(&self.shared.stack_rows));
        let lease = match instances.state(owner) {
            Ok(InstanceState::Running(lease)) if allowed => Some(lease),
            _ => None,
        };
        // The tap leaves the queue before its callback runs, so it is tried
        // once whatever the callback does (fails, traps, drops its own
        // instance). A table in use leaves it for the next pass.
        if ui.widget_input_delivered(&input).is_err() {
            return false;
        }
        if let Some(lease) = lease {
            // A failing callback is not told again.
            let _ = self.native_input_call(owner, &lease, input.token, &record[..len]);
        }
        true
    }
}

#[cfg(test)]
#[path = "widget_inputs_tests.rs"]
mod tests;
