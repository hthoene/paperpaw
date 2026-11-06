//! Owner-admitted callbacks selected by declared screen or action tokens.
use super::registrations;
use super::{
    Batch, DispatchDepth, Error, Registration, borrowed, host_glue, no_reply, status_only,
};
use paperpaw_graphics::Canvas;

/// A host-produced event or input for a declared screen or action token.
/// Tokens come from committed declarations, never a fabricated registration.
#[allow(
    dead_code,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) enum TokenInvocation<'a> {
    Event(&'a [u8]),
    Input(&'a [u8]),
}

/// Dispatch a token resolved by the host from one committed declaration.
/// The frame remains scoped to this synchronous callback and owner admission.
#[allow(
    dead_code,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) fn dispatch_token<S, M>(
    batch: &Batch<'_, S, M, Registration>,
    token: u32,
    depth: &DispatchDepth,
    frame: &crate::abi7::core_frame::CoreFrame<'_>,
    invocation: TokenInvocation<'_>,
) -> Result<i32, Error> {
    batch.dispatch_owner(depth, |context| {
        let instance = context.instance.get().ok_or(Error::Attempt)?;
        // SAFETY: the owner structurally pins the field; nothing moves it.
        let scope = unsafe { context.map_unchecked(|value| &value.environment) };
        scope.scope_core(frame, || match invocation {
            TokenInvocation::Event(bytes) => {
                let callback = instance
                    .event
                    .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
                // SAFETY: the admitted owner retains state and code. The
                // host-produced payload is borrowed only until this returns.
                Ok(status_only(unsafe {
                    callback(instance.state, token, borrowed(bytes), no_reply())
                }))
            }
            TokenInvocation::Input(bytes) => {
                let Some(callback) = instance.input else {
                    return Ok(0);
                };
                // SAFETY: the admitted owner retains state and code, and this
                // host-produced input is borrowed only for the callback.
                let status = unsafe { callback(instance.state, token, borrowed(bytes)) };
                Ok(if status > 1 {
                    host_glue::ERR_INVALID
                } else {
                    status
                })
            }
        })?
    })?
}

/// Draw a declared screen token with its full pane and separate clip rows.
/// The adapter supplies the complete frame rather than deriving a pane from
/// the clipped canvas, which may hold only one band of a larger screen.
#[allow(
    dead_code,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) fn dispatch_token_draw<S, M>(
    batch: &Batch<'_, S, M, Registration>,
    token: u32,
    depth: &DispatchDepth,
    frame: &crate::abi7::core_frame::CoreFrame<'_>,
    draw_frame: &registrations::DrawFrame<'_>,
    canvas: &mut Canvas<'_>,
) -> Result<i32, Error> {
    if canvas.clip().width == 0 || canvas.clip().height == 0 {
        return Ok(host_glue::OK);
    }
    let mut bytes = [0; 320];
    let len = super::super::frames::encode(draw_frame, &mut bytes).map_err(Error::Status)?;
    batch.dispatch_owner(depth, |context| {
        let instance = context.instance.get().ok_or(Error::Attempt)?;
        let callback = instance
            .draw
            .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
        // SAFETY: the owner structurally pins these fields; nothing moves them.
        let env_scope = unsafe { context.map_unchecked(|value| &value.environment) };
        let canvas_scope = unsafe { context.map_unchecked(|value| &value.canvas) };
        env_scope
            .scope_core(frame, || {
                canvas_scope.scope(canvas, || {
                    // SAFETY: the admitted owner retains state and code. Encoded
                    // bytes and the canvas scope live only for this callback.
                    status_only(unsafe { callback(instance.state, token, borrowed(&bytes[..len])) })
                })
            })?
            .map_err(Into::into)
    })?
}
