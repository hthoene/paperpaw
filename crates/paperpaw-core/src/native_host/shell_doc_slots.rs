//! The host slots of images, hyphenation and icons (36, 37, 38, 40): the C
//! entry points the host table names, each a thin hop into its service.
use super::{Context, PpBuffer, PpBytes, PpHost, c_void, host_glue};
use crate::native_host::{fonts, host_slot, hyphen_service, image_service};
use paperpaw_native::host_glue::Services;

/// Names the four entry points in the host table.
pub(super) const fn install<S: Services + 'static>(table: &mut PpHost) {
    table.text_hyphenate = text_hyphenate::<S>;
    table.icon_glyph = icon_glyph::<S>;
    table.image_info = image_info::<S>;
    table.canvas_draw_image = canvas_draw_image::<S>;
}

pub(super) unsafe extern "C" fn image_info<S: 'static>(
    ctx: *mut c_void,
    image: u32,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    // SAFETY: the generated slot contract supplies a live output pair.
    unsafe { image_service::info_slot(&context.resources, image, out) }
}

pub(super) unsafe extern "C" fn canvas_draw_image<S: 'static>(
    ctx: *mut c_void,
    image: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    flags: u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    image_service::draw_slot(
        &context.canvas,
        &context.environment,
        &context.resources,
        image,
        x,
        y,
        width,
        height,
        flags,
    )
}

pub(super) unsafe extern "C" fn icon_glyph<S: 'static>(
    ctx: *mut c_void,
    name: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe { fonts::icon_glyph(&context.environment, name, out) }
}

pub(super) unsafe extern "C" fn text_hyphenate<S: 'static>(
    ctx: *mut c_void,
    language: PpBytes,
    word: PpBytes,
    left: u32,
    right: u32,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input and output pairs.
    unsafe { hyphen_service::slot(&context.resources, language, word, left, right, out) }
}
