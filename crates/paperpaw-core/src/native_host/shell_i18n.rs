//! The text entries of the host table (`i18n.copy`, `i18n.format`,
//! `i18n.digits`): the plugin's catalog through its environment scope.
use super::Context;
use crate::native_host::{formatting, host_slot};
use core::ffi::c_void;
use paperpaw_native::host_glue::{self, PpBuffer, PpBytes};

pub(super) unsafe extern "C" fn copy<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe { formatting::copy(&context.environment, key, out) }
}

pub(super) unsafe extern "C" fn format<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    args: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe { formatting::format(&context.environment, key, args, out) }
}

pub(super) unsafe extern "C" fn digits<S: 'static>(
    ctx: *mut c_void,
    text: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe { formatting::digits(&context.environment, text, out) }
}
