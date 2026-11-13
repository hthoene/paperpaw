//! `event.widget_made` (84) and `event.widget_dropped` (85) to the owners of
//! widget instances other plugins made and dropped (`ui.md` 5.2 and 8.2): the
//! owner hears of each at the widget's token, of a make before its drop.
//!
//! A made instance is told here between callbacks, or inside its maker's
//! callback right before the owner is first called about it ([`tell_made`]:
//! a `core.widget` call or a nested draw), so the owner knows it then.
//!
//! A notice stays listed until the first delivery attempt is over, whatever
//! came of it (the callback returned, failed or trapped): there is no retry.
//! An owner that cannot be called (stopped, unloaded, switched off) loses its
//! notices without a delivery, so a dead owner never pins the widget cap. A
//! buffer too small for the record leaves the notice listed and ends the pass.

use super::codec;
use super::native_instances::InstanceState;
use super::native_screen::Dispatch;
use super::registrations::{self as wire, Record};
use super::ui_handles::{Dropped, Pick, UiHandles};
use core::cell::RefCell;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::Env;

/// The room of one record of event 85: a batch of one, the largest values
/// included, takes well under half of it (the test
/// `the_record_fits_its_buffer_and_a_buffer_too_small_leaves_the_notice`).
const RECORD_BYTES: usize = 64;

/// Notices told in one pass: the most the device-wide widget table holds.
const PER_PASS: usize = 32;

/// The room of one record of event 84: a batch of one with a widget id and a
/// place of 128 bytes each fits (the test
/// `the_largest_made_record_fits_its_buffer`).
pub(crate) const MADE_BYTES: usize = 320;

/// Tells the owner (its `lease`, about to be called about `instance`) the
/// instance was made, if it has not heard of it: inside the maker's callback,
/// at the depth the owner's call runs at. The notice ends at this attempt,
/// whatever came of it. Its own frame, so the record's buffer is gone before
/// the owner's call.
#[inline(never)]
pub(crate) fn tell_made(
    handles: &RefCell<UiHandles>,
    lease: &dyn Dispatch,
    instance: u32,
    env: &Env<'_>,
) {
    let mut record = [0u8; MADE_BYTES];
    let Ok(Some((notice, len))) = made_record(
        handles,
        Pick::Instance(instance),
        env.now().millis(),
        &mut record,
    ) else {
        return;
    };
    // The outcome is the plugin's own: the notice ends either way.
    let _ = lease.event(notice.token, record.get(..len).unwrap_or(&[]), env);
    made_told(handles, instance);
}

/// The `event.widget_made` record of the notice `pick` names, for its
/// owner's token (ui.md 5.2), in `out`: the notice and the length; `None`
/// when it does not wait. `ERR_BUSY` while the table is in use.
pub(super) fn made_record(
    handles: &RefCell<UiHandles>,
    pick: Pick,
    now_ms: u64,
    out: &mut [u8],
) -> Result<Option<(Dropped, usize)>, i32> {
    let handles = handles.try_borrow().map_err(|_| ERR_BUSY)?;
    let Some(made) = handles.next_made(pick) else {
        return Ok(None);
    };
    let record = Record::EventWidgetMade(wire::EventWidgetMade {
        now_ms,
        widget: made.widget(),
        instance: made.instance,
        place: made.place(),
    });
    let len = codec::encode_batch(&[record], out)?;
    let notice = Dropped {
        owner: made.owner,
        token: made.token,
        instance: made.instance,
    };
    Ok(Some((notice, len)))
}

/// The first attempt to tell of `instance` is over; a table in use keeps the
/// notice for the next pass.
fn made_told(handles: &RefCell<UiHandles>, instance: u32) -> bool {
    handles
        .try_borrow_mut()
        .map(|mut handles| handles.made_told(instance))
        .is_ok()
}

impl crate::Kernel {
    /// Tells the owners of made widget instances, oldest first, outside of
    /// any native callback. An owner not started yet keeps its notice and is
    /// asked to start (told on a later pass, once it runs); one that cannot be
    /// called otherwise (stopped, gone) loses it. `true` when a plugin was
    /// called (what it staged is the next round's).
    // Its own frame: not inlined into `run_commands` (#250).
    #[inline(never)]
    pub(crate) fn deliver_widget_made(&mut self) -> bool {
        let mut called = false;
        // Notices kept for owners not started yet come first in the list.
        let mut kept = 0;
        for _ in 0..PER_PASS {
            let instances = &self.shared.native_instances;
            if instances.depth() != 0 {
                break;
            }
            let Some(ui) = instances.ui_services() else {
                break;
            };
            let handles = &ui.widgets.handles;
            let mut record = [0u8; MADE_BYTES];
            let now = self.now;
            let Ok(Some((notice, len))) =
                made_record(handles, Pick::Nth(kept), now.millis(), &mut record)
            else {
                // Nothing (more) waits, or the table is in use: the next pass tries.
                break;
            };
            match instances.state(notice.owner) {
                Ok(InstanceState::Running(lease)) => {
                    let env = self.shared.env(now);
                    let _ = lease.event(notice.token, record.get(..len).unwrap_or(&[]), &env);
                    called = true;
                }
                // Not started yet: started, and told on a later pass.
                Ok(InstanceState::Lazy) if instances.request_start(notice.owner).is_ok() => {
                    kept += 1;
                    continue;
                }
                // Stopped or gone: nobody to tell.
                _ => {}
            }
            if !made_told(handles, notice.instance) {
                break;
            }
        }
        called
    }

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
