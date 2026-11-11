//! Slot 39 (`text.covers`): the shell's entry over [`super::super::fonts::covers`].
use super::Context;
use core::ffi::c_void;
use paperpaw_native::host_glue::{ERR_INVALID, PpBytes};

/// # Safety
/// The context is the pinned shell `Context` supplied to `make`; the text
/// pair is live and readable for this call.
pub(super) unsafe extern "C" fn covers<S: 'static>(
    ctx: *mut c_void,
    handle: u32,
    text: PpBytes,
) -> i32 {
    // SAFETY: a nonnull context is the pinned shell Context supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = crate::native_host::host_slot::enter();
    // SAFETY: the slot contract supplies live, readable input.
    unsafe {
        crate::native_host::fonts::covers(&context.environment, &context.resources, handle, text)
    }
}
