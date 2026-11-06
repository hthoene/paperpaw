//! Scoped font lookup and scalar text slots. Persistent handles own keys only.
use super::{
    canvas::CanvasScope,
    environment::EnvironmentScope,
    resources::{FontKey, FontRole, OwnerResources},
};
use crate::abi7::{context::Frame, graphics};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, OK, PpBytes, buffers,
};
use paperpaw_platform::Color;
use paperpaw_sdk::{FontHandle, Fonts, builtin::fonts as roles};

fn role(key: &str) -> Option<FontRole> {
    Some(match key {
        "role:ui" => FontRole::Ui,
        "role:reading" => FontRole::Reading,
        "role:reading-bold" => FontRole::ReadingBold,
        "role:reading-italic" => FontRole::ReadingItalic,
        "role:reading-bold-italic" => FontRole::ReadingBoldItalic,
        "role:status" => FontRole::Status,
        "role:icons" => FontRole::Icons,
        _ => return None,
    })
}

fn resolve_role(fonts: &Fonts, role: FontRole) -> Option<FontHandle<'_>> {
    match role {
        FontRole::Ui => fonts.role(roles::UI.as_str()),
        FontRole::Reading => fonts.role(roles::READING.as_str()),
        FontRole::ReadingBold => fonts
            .bound(roles::READING_BOLD.as_str())
            .or_else(|| resolve_role(fonts, FontRole::Reading)),
        FontRole::ReadingItalic => fonts
            .bound(roles::READING_ITALIC.as_str())
            .or_else(|| resolve_role(fonts, FontRole::Reading)),
        FontRole::ReadingBoldItalic => fonts
            .bound(roles::READING_BOLD_ITALIC.as_str())
            .or_else(|| resolve_role(fonts, FontRole::ReadingBold)),
        FontRole::Status => fonts.role(roles::STATUS.as_str()),
        FontRole::Icons => fonts.bound(roles::ICONS.as_str()),
    }
}

fn resolve<'a>(frame: &'a dyn Frame, key: &FontKey) -> Result<FontHandle<'a>, i32> {
    let fonts = frame.fonts().ok_or(ERR_STATE)?;
    match key {
        FontKey::Role(role) => resolve_role(fonts, *role).ok_or(ERR_NOT_FOUND),
        FontKey::Named { id, acquired } => {
            let tracker = frame.font_removals().ok_or(ERR_STATE)?;
            if !tracker.valid(id.namespace(), *acquired) {
                return Err(ERR_STATE);
            }
            fonts.get(id.as_str()).ok_or(ERR_STATE)
        }
    }
}

fn with_font<R>(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    call: impl FnOnce(&FontHandle<'_>) -> Result<R, i32>,
) -> Result<R, i32> {
    scope.with_frame(|frame| owner.with_font(handle, |key| call(&resolve(frame, key)?)))?
}

/// Validate scalar output ranges without borrowing or initializing their storage.
fn outputs<T, const N: usize>(pointers: [*mut T; N]) -> Result<(), i32> {
    let bytes = core::mem::size_of::<T>();
    for (index, pointer) in pointers.iter().enumerate() {
        let start = *pointer as usize;
        let end = start.checked_add(bytes).ok_or(ERR_INVALID)?;
        if pointer.is_null() || !start.is_multiple_of(core::mem::align_of::<T>()) {
            return Err(ERR_INVALID);
        }
        if pointers[..index].iter().any(|earlier| {
            let earlier = *earlier as usize;
            earlier < end && start < earlier.saturating_add(bytes)
        }) {
            return Err(ERR_INVALID);
        }
    }
    Ok(())
}

fn size_check(size: f32) -> Result<(), i32> {
    if size.is_finite() && size > 0.0 {
        Ok(())
    } else {
        Err(ERR_INVALID)
    }
}

fn status(result: Result<(), i32>) -> i32 {
    result.map_or_else(|error| error, |()| OK)
}

/// Acquire slot 7. The key is copied only for a named font, using checked allocation.
///
/// # Safety
/// Input must be readable and stable; output must be writable, exclusive and
/// disjoint from input and host state. Output may be uninitialized.
pub(crate) unsafe fn acquire(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    key: PpBytes,
    out: *mut u32,
) -> i32 {
    status((|| {
        outputs([out])?;
        // SAFETY: caller guarantees readable input for this synchronous call.
        let key = unsafe { buffers::text_in(key, 128) }?;
        let key = scope.with_frame(|frame| {
            let fonts = frame.fonts().ok_or(ERR_STATE)?;
            if key.starts_with("role:") {
                let role = role(key).ok_or(ERR_NOT_FOUND)?;
                if !matches!(role, FontRole::Icons) && resolve_role(fonts, role).is_none() {
                    return Err(ERR_NOT_FOUND);
                }
                return Ok(FontKey::Role(role));
            }
            let acquired = frame
                .font_removals()
                .and_then(|tracker| tracker.generation())
                .ok_or(ERR_STATE)?;
            let font = fonts.get(key).ok_or(ERR_NOT_FOUND)?;
            let id = font.id().try_clone().ok_or(ERR_NO_MEMORY)?;
            Ok(FontKey::Named { id, acquired })
        })??;
        let handle = owner.insert_font(key)?;
        // SAFETY: validated scalar output; no fallible step follows publication.
        unsafe { out.write(handle) };
        Ok(())
    })())
}

/// Slot 8 is valid even outside an environment callback.
pub(crate) fn release(owner: &OwnerResources, handle: u32) -> i32 {
    status(owner.release(handle))
}

/// Slot 13: measure one line, writing its advance only on success.
///
/// # Safety
/// Text is readable and stable; output is exclusively writable and disjoint
/// from text and host state for the call. It may be uninitialized.
pub(crate) unsafe fn measure(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    size: f32,
    text: PpBytes,
    out: *mut f32,
) -> i32 {
    status((|| {
        outputs([out])?;
        size_check(size)?;
        // SAFETY: the caller provides live readable input.
        let text = unsafe { buffers::text_in(text, buffers::TEXT_MAX) }?;
        let width = with_font(scope, owner, handle, |font| {
            graphics::text_measure(font, size, text).map_err(|_| ERR_INVALID)
        })?;
        // SAFETY: output was validated and no reference to it was formed.
        unsafe { out.write(width) };
        Ok(())
    })())
}

/// Slot 29: publish all three metrics together, including positive descent.
///
/// # Safety
/// Outputs must be live, writable, exclusive and disjoint from host state.
/// Their numeric ranges must not overlap; they may be uninitialized.
#[allow(clippy::too_many_arguments, reason = "matches the ABI metrics scalars")]
pub(crate) unsafe fn metrics(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    size: f32,
    ascent: *mut f32,
    descent: *mut f32,
    height: *mut f32,
) -> i32 {
    status((|| {
        outputs([ascent, descent, height])?;
        size_check(size)?;
        let values = with_font(scope, owner, handle, |font| {
            let metrics = font.line_metrics(size);
            let values = [metrics.ascent, metrics.descent, metrics.line_height];
            if values
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0)
            {
                Ok(values)
            } else {
                Err(ERR_INVALID)
            }
        })?;
        // SAFETY: every output was validated before resolving the font.
        unsafe {
            ascent.write(values[0]);
            descent.write(values[1]);
            height.write(values[2]);
        }
        Ok(())
    })())
}

#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
fn ink_of(_font: &FontHandle<'_>, _size: f32, _text: &str) -> Result<[i32; 4], i32> {
    // Kernel supplies ink_drawn, sharing the shaped raster path with draw.
    Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
}

/// Slot 30: validate and resolve before probing the exact shaped raster path.
///
/// # Safety
/// Text must be readable and unchanged. All outputs must be writable, exclusive
/// and disjoint from text and host state. They may be uninitialized.
#[allow(clippy::too_many_arguments, reason = "matches the ABI ink scalars")]
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
pub(crate) unsafe fn ink(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    size: f32,
    text: PpBytes,
    top: *mut i32,
    bottom: *mut i32,
    left: *mut i32,
    right: *mut i32,
) -> i32 {
    status((|| {
        outputs([top, bottom, left, right])?;
        size_check(size)?;
        // SAFETY: the caller provides live readable input.
        let text = unsafe { buffers::text_in(text, buffers::TEXT_MAX) }?;
        let values = with_font(scope, owner, handle, |font| ink_of(font, size, text))?;
        // SAFETY: all outputs were checked before any write.
        unsafe {
            top.write(values[0]);
            bottom.write(values[1]);
            left.write(values[2]);
            right.write(values[3]);
        }
        Ok(())
    })())
}

/// Slot 14: draw through the original canvas's current clip and semantic sink.
///
/// # Safety
/// Text must be readable and unchanged. Output must be writable, exclusive and
/// disjoint from text and host state for the call; it may be uninitialized.
#[allow(
    clippy::too_many_arguments,
    reason = "matches the ABI text draw scalars"
)]
pub(crate) unsafe fn draw(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    canvas: &CanvasScope,
    handle: u32,
    size: f32,
    x: f32,
    baseline: i32,
    color: u32,
    text: PpBytes,
    out: *mut f32,
) -> i32 {
    status((|| {
        outputs([out])?;
        size_check(size)?;
        if color >> 24 != 0 {
            return Err(ERR_INVALID);
        }
        let [_, red, green, blue] = color.to_be_bytes();
        let color = Color::rgb(red, green, blue);
        // SAFETY: the caller provides live readable input.
        let text = unsafe { buffers::text_in(text, buffers::TEXT_MAX) }?;
        let width = with_font(scope, owner, handle, |font| {
            canvas.with_canvas(|canvas| {
                graphics::text_draw(canvas, font, size, x, i64::from(baseline), text, color)
                    .map_err(|_| ERR_INVALID)
            })?
        })?;
        // SAFETY: output was validated before drawing.
        unsafe { out.write(width) };
        Ok(())
    })())
}

#[cfg(test)]
#[path = "fonts_tests.rs"]
mod tests;
