//! Slot 9 (`canvas.fill_rect`) forwarding for the inactive owner integration.
//!
//! The foreign callback reaches Kernel's [`CanvasScope`] only inside one
//! synchronous draw dispatch. Colour is generated `0x00RRGGBB`; this module
//! forwards the scalars unchanged and does no conversion of its own.
use super::*;
use paperpaw_core::sdk::Env;
use paperpaw_graphics::Canvas;

pub(super) unsafe extern "C" fn fill_rect<S>(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: u32,
) -> i32 {
    // SAFETY: a nonnull context is exactly the pinned Context<S> given to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    context.canvas.fill_rect_raw(x, y, width, height, color)
}

/// Draw dispatch with a frame: the Env (slots 4/5) and the Canvas (slot 9)
/// are both borrowed for this one synchronous callback and cleared on every
/// exit, including unwind. Nested installation is refused by the scopes.
pub(super) fn dispatch_draw<S>(
    batch: &Batch<'_, S, Registration>,
    index: usize,
    depth: &DispatchDepth,
    env: &Env<'_>,
    canvas: &mut Canvas<'_>,
) -> Result<i32, Error> {
    // The screen frame (N2): the canvas's visible region, drawn in one pass.
    let clip = canvas.clip();
    let frame = paperpaw_core::native_host::frames::screen(
        i32::from(clip.x),
        i32::from(clip.y),
        u32::from(clip.width),
        u32::from(clip.height),
        (
            i32::from(clip.y),
            i32::from(clip.y) + i32::from(clip.height),
        ),
    );
    let mut bytes = [0; 64];
    let len =
        paperpaw_core::native_host::frames::encode(&frame, &mut bytes).map_err(Error::Status)?;
    let frame = &bytes[..len];
    batch.dispatch(index, depth, |context, record| {
        // SAFETY: the owner structurally pins these fields; no API moves them.
        let env_scope = unsafe { context.map_unchecked(|value| &value.environment) };
        let canvas_scope = unsafe { context.map_unchecked(|value| &value.canvas) };
        env_scope.scope(env, || {
            canvas_scope.scope(canvas, || {
                invoke_registration(context, *record, Invocation::Draw(frame))
            })
        })??
    })?
}
