//! A native plugin's action: the host runs it by delivering `event.action_run`
//! to the token the plugin declared (`docs/abi/screens-input.md` section 2).
//!
//! The handler keeps the plugin's lease, so [`Actions::remove_plugin`] must
//! drop it when the plugin unloads (`begin_native_unload`). A stopped plugin's
//! lease refuses the event.
//!
//! The handler runs inside the kernel's action dispatch, where no command
//! stage is at hand: it only queues the event ([`ActionQueue`]). The kernel
//! delivers it from its pump, between callbacks (`Kernel::native_actions_run`),
//! with the callback's command stage, as for every other callback, so that
//! what the plugin asks (`open` its screen) reaches the kernel in the same
//! call that ran the action. The plugin's answer therefore comes after the
//! rest of the dispatch that ran the action (what other plugins' observers
//! asked for), not inside it.

use super::{
    native_instances::NativeLease,
    native_values::Callback,
    registrations::{EventActionRun, Record},
    shared::try_box,
};
use alloc::{borrow::Cow, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NO_SPACE};
use paperpaw_sdk::{ActionHandler, ActionSpec};

/// The longest action id `event.action_run` carries.
const EVENT_BYTES: usize = 192;

/// Actions queued between two passes of the kernel's pump. A button or a
/// script runs one at a time; an action that runs others (a rule) stays within
/// it. One more is refused (the action has no effect, as a stopped plugin's).
const QUEUE_MAX: usize = 4;

/// One `event.action_run` waiting for the kernel to deliver it.
pub(crate) struct Pending {
    /// The plugin's key.
    pub(crate) owner: u64,
    pub(crate) lease: NativeLease,
    /// The token the plugin declared the action with.
    pub(crate) token: u32,
    /// The encoded one-record event.
    pub(crate) bytes: Vec<u8>,
}

/// The actions run and not yet delivered, in order, and how many were
/// refused for want of room or memory since the kernel last said so.
#[derive(Default)]
pub(crate) struct ActionQueue {
    waiting: RefCell<Vec<Pending>>,
    refused: core::cell::Cell<u32>,
}

impl ActionQueue {
    /// Keeps `pending` for the kernel; a refusal is counted ([`Self::refused`]).
    pub(crate) fn push(&self, pending: Pending) -> Result<(), i32> {
        let kept = self.keep(pending);
        if kept.is_err() {
            self.refused.set(self.refused.get().saturating_add(1));
        }
        kept
    }

    /// The refusals since the last call.
    pub(crate) fn refused(&self) -> u32 {
        self.refused.take()
    }

    fn keep(&self, pending: Pending) -> Result<(), i32> {
        let mut queue = self.waiting.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if queue.len() >= QUEUE_MAX {
            return Err(ERR_NO_SPACE);
        }
        queue.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
        queue.push(pending);
        Ok(())
    }

    /// The oldest action waiting.
    pub(crate) fn take(&self) -> Option<Pending> {
        let mut queue = self.waiting.try_borrow_mut().ok()?;
        (!queue.is_empty()).then(|| queue.remove(0))
    }
}

/// The action `callback` declares, run through `lease` of the plugin `owner`
/// by way of the queue of `instances`.
///
/// # Errors
/// `ERR_NO_MEMORY` when the id, the handler or its box cannot be allocated.
pub(crate) fn spec(
    callback: Callback,
    lease: NativeLease,
    owner: u64,
    instances: &super::native_instances::NativeInstances,
) -> Result<ActionSpec, i32> {
    let queue = instances.action_queue()?;
    let Callback {
        id, title, token, ..
    } = callback;
    let name = id.try_clone().ok_or(ERR_NO_MEMORY)?;
    let run = move |env: &paperpaw_sdk::Env<'_>,
                    _: &paperpaw_sdk::Params,
                    _: &mut paperpaw_sdk::Commands| {
        let mut bytes = [0; EVENT_BYTES];
        let record = Record::EventActionRun(EventActionRun {
            now_ms: env.now().millis(),
            action: name.as_str(),
            // Bit 0 `background`: not the reader's own doing (a rule's or
            // another plugin's run), as interface 6 `Env::reader` said.
            flags: u32::from(!env.reader()),
        });
        if let Ok(len) = super::events::encode(&record, &mut bytes) {
            // Memory or a full queue leaves the action without effect, as a
            // stopped or failing plugin does; the pump reports the failure of
            // the callback.
            let mut held = Vec::new();
            if held.try_reserve_exact(len).is_ok()
                && let Ok(lease) = lease.try_clone()
            {
                held.extend_from_slice(&bytes[..len]);
                let _ = queue.push(Pending {
                    owner,
                    lease,
                    token,
                    bytes: held,
                });
            }
        }
    };
    let handler: ActionHandler = try_box(run).ok_or(ERR_NO_MEMORY)?;
    let mut action = ActionSpec::new(id, Cow::Owned(title));
    action.handler = Some(handler);
    Ok(action)
}

#[cfg(test)]
#[path = "native_action_tests.rs"]
mod tests;
