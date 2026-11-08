//! A native plugin's action: the host runs it by delivering `event.action_run`
//! to the token the plugin declared (`docs/abi/screens-input.md` section 2).
//!
//! The handler keeps the plugin's lease, so [`Actions::remove_plugin`] must
//! drop it when the plugin unloads (`begin_native_unload`). A stopped plugin's
//! lease refuses the event.

use super::{
    native_instances::NativeLease,
    native_screen::Dispatch,
    native_values::Callback,
    registrations::{EventActionRun, Record},
    shared::try_box,
};
use alloc::borrow::Cow;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_sdk::{ActionHandler, ActionSpec};

/// The longest action id `event.action_run` carries.
const EVENT_BYTES: usize = 192;

/// The action `callback` declares, run through `lease`.
///
/// # Errors
/// `ERR_NO_MEMORY` when the id, the handler or its box cannot be allocated.
pub(crate) fn spec(callback: Callback, lease: NativeLease) -> Result<ActionSpec, i32> {
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
        });
        if let Ok(len) = super::events::encode(&record, &mut bytes) {
            // A stopped or failing plugin leaves the action without effect;
            // the dispatch reports its own failure.
            let _ = lease.event(token, &bytes[..len], env);
        }
    };
    let handler: ActionHandler = try_box(run).ok_or(ERR_NO_MEMORY)?;
    let mut action = ActionSpec::new(id, Cow::Owned(title));
    action.handler = Some(handler);
    Ok(action)
}
