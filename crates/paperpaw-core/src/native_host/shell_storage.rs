//! The storage slots 20-26 of a hosted plugin: the raw `extern "C"` entries
//! the host table points to, each over [`storage_slots`](super::super::storage_slots).
//! A child of the shell, which keeps the plugin's context private.
use super::{Context, PpBuffer, PpBytes, c_void, host_glue};

pub(super) unsafe extern "C" fn storage_size<S>(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    out: *mut u64,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe {
        super::super::storage_slots::size(
            &context.environment,
            &context.id,
            format_of(context),
            root,
            path,
            out,
        )
    }
}

pub(super) unsafe extern "C" fn storage_read_at<S>(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe {
        super::super::storage_slots::read_at(
            &context.environment,
            &context.id,
            format_of(context),
            root,
            path,
            offset,
            out,
        )
    }
}

/// What lets a storage slot of this plugin name the document its format call
/// is for (roots 3, 4 and 6).
fn format_of<S>(context: &Context<S>) -> Option<super::super::storage_slots::Format<'_>> {
    Some(super::super::storage_slots::Format {
        scope: context.resources.hosts().format_scope(),
        owner: context.resources.owner_id(),
    })
}

pub(super) unsafe extern "C" fn storage_write<S>(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    data: PpBytes,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    let held = context.resources.hosts().in_use();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe {
        super::super::storage_slots::write(
            &context.environment,
            &context.id,
            format_of(context),
            held,
            root,
            path,
            data,
        )
    }
}

pub(super) unsafe extern "C" fn storage_append<S>(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    data: PpBytes,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    let held = context.resources.hosts().in_use();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe {
        super::super::storage_slots::append(
            &context.environment,
            &context.id,
            format_of(context),
            held,
            root,
            path,
            data,
        )
    }
}

pub(super) unsafe extern "C" fn storage_rename<S>(
    ctx: *mut c_void,
    root: u32,
    from: PpBytes,
    to: PpBytes,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    let held = context.resources.hosts().in_use();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe {
        super::super::storage_slots::rename(
            &context.environment,
            &context.id,
            format_of(context),
            held,
            root,
            from,
            to,
        )
    }
}

pub(super) unsafe extern "C" fn storage_remove<S>(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    let held = context.resources.hosts().in_use();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe {
        super::super::storage_slots::remove(
            &context.environment,
            &context.id,
            format_of(context),
            held,
            root,
            path,
        )
    }
}

pub(super) unsafe extern "C" fn storage_sync<S>(ctx: *mut c_void) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::super::host_slot::enter();
    super::super::storage_slots::sync(&context.environment, &context.id)
}
