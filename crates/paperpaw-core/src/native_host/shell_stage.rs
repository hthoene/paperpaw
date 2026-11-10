//! Bounded stage replies through the existing retained owner admission path.
use super::{Batch, DispatchDepth, Error, Registration, borrowed};
use crate::abi7::core_frame::CoreFrame;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED, PpBuffer};

/// The one nested dispatch path: a plugin's callback reached from inside another
/// plugin's (a stage subscriber, a compose child). The callee is entered on the
/// frame's usage meter for the call, so time, storage and network are charged to
/// the plugin that runs and the secrets and Bluetooth views of the `Env` name it.
/// Access decisions never read that tracker: they use the caller's identity.
///
/// Dispatch only a token resolved from a committed stage registration. The
/// retained batch keeps the pinned host, plugin state and module live. Membership,
/// lazy start, framing and publication belong to the stage broker.
/// Negative statuses describe a delivered callback, not an admission failure.
#[allow(
    dead_code,
    reason = "stage registry and frozen-membership broker follow this hook"
)]
pub(crate) fn dispatch_stage<S: 'static, M>(
    batch: &Batch<'_, S, M, Registration>,
    token: u32,
    depth: &DispatchDepth,
    frame: &CoreFrame<'_>,
    input: &[u8],
    output: &mut [u8],
) -> Result<i32, Error> {
    batch.dispatch_owner(depth, |context| {
        let _running = frame.meter.enter(&context.id);
        let instance = context.instance.get().ok_or(Error::Attempt)?;
        let callback = instance.event.ok_or(Error::Status(ERR_UNSUPPORTED))?;
        // SAFETY: the retained owner structurally pins this field and keeps it
        // alive until the batch lease is dropped before the owner and module.
        let scope = unsafe { context.map_unchecked(|value| &value.environment) };
        scope
            .scope_core(frame, || {
                let capacity = output.len();
                // SAFETY: owner admission precedes all access to plugin state. Input
                // and exclusive output live for this call and do not alias host state.
                let result = context.run_plugin(|| unsafe {
                    callback(
                        instance.state,
                        token,
                        borrowed(input),
                        PpBuffer {
                            ptr: output.as_mut_ptr(),
                            cap: capacity,
                        },
                    )
                });
                if result >= 0 && result as usize > capacity {
                    ERR_INVALID
                } else {
                    result
                }
            })
            .map_err(Error::from)
    })?
}
