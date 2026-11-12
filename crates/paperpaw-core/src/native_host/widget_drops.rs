//! `event.widget_dropped` (85) to the owners of dropped widget instances
//! (`ui.md` 5.2 and 8.2): the maker hears of each drop at the widget's token.
//!
//! A notice stays listed until the first delivery attempt is over, whatever
//! came of it (the callback returned, failed or trapped): there is no retry.
//! An owner that cannot be called (stopped, unloaded, switched off) loses its
//! notices without a delivery, so a dead owner never pins the widget cap. A
//! buffer too small for the record leaves the notice listed and ends the pass.

use super::native_instances::InstanceState;
use super::native_screen::Dispatch;
use paperpaw_native::host_glue::ERR_BUSY;

/// The room of one record of event 85: a batch of one, the largest values
/// included, takes well under half of it (the test
/// `the_record_fits_its_buffer_and_a_buffer_too_small_leaves_the_notice`).
const RECORD_BYTES: usize = 64;

/// Notices told in one pass: the most the device-wide widget table holds.
const PER_PASS: usize = 32;

impl crate::Kernel {
    /// Tells the owners of dropped widget instances, oldest first, outside of
    /// any native callback. `true` when a plugin was called (what it staged is
    /// for the next round of commands).
    // Its own frame: not inlined into `run_commands` (#250).
    #[inline(never)]
    pub(crate) fn deliver_widget_drops(&mut self) -> bool {
        let mut called = false;
        for _ in 0..PER_PASS {
            if self.shared.native_instances.depth() != 0 {
                break;
            }
            let Some(ui) = self.shared.native_instances.ui_services() else {
                break;
            };
            let Ok(Some(owner)) = ui.next_dropped_owner() else {
                break;
            };
            let mut record = [0u8; RECORD_BYTES];
            let now = self.now;
            let (notice, len) = match ui.next_widget_dropped(owner, now.millis(), &mut record) {
                Ok(Some(told)) => told,
                other => {
                    // Nothing for this owner after all, or the table is in
                    // use (`ERR_BUSY`): the notice stays, the next pass tries
                    // again. A record that does not fit the buffer is a bug.
                    debug_assert!(
                        other.is_ok() || other == Err(ERR_BUSY),
                        "an event.widget_dropped record fits {RECORD_BYTES} bytes: {other:?}"
                    );
                    break;
                }
            };
            if let Ok(InstanceState::Running(lease)) = self.shared.native_instances.state(owner) {
                let env = self.shared.env(now);
                // The outcome is the plugin's own: the entry ends either way.
                let _ = lease.event(notice.token, &record[..len], &env);
                called = true;
            }
            if ui.widget_dropped_delivered(&notice).is_err() {
                break;
            }
        }
        called
    }
}
