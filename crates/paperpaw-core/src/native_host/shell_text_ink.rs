//! Host slot 30, `text.ink`. The X4 and the X4 Pro wire it only with the core
//! feature `x4-ink` until device timing qualifies it there
//! (`docs/abi/screens-input.md`); every other host always does.

use super::Context;
use core::ffi::c_void;
use paperpaw_native::host_glue::{self, PpBytes};

#[allow(clippy::too_many_arguments, reason = "matches the ABI scalar slot")]
pub(super) unsafe extern "C" fn text_ink<S: 'static>(
    ctx: *mut c_void,
    handle: u32,
    size: f32,
    text: PpBytes,
    top: *mut i32,
    bottom: *mut i32,
    left: *mut i32,
    right: *mut i32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = crate::native_host::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe {
        crate::native_host::fonts::ink(
            &context.environment,
            &context.resources,
            handle,
            size,
            text,
            top,
            bottom,
            left,
            right,
        )
    }
}
