//! Scoped font lookup and scalar text slots. Persistent handles own keys only.
use super::{
    canvas::CanvasScope,
    environment::EnvironmentScope,
    fonts_data::DataFont,
    resources::{FontKey, FontRole, OwnerResources},
};
use crate::abi7::{context::Frame, graphics};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, OK, PpBuffer, PpBytes,
    buffers,
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

fn resolve<'a>(frame: &'a dyn Frame, key: &'a FontKey) -> Result<FontHandle<'a>, i32> {
    if let FontKey::Data(font) = key {
        return font.handle();
    }
    let fonts = frame.fonts().ok_or(ERR_STATE)?;
    match key {
        FontKey::Role(role) => resolve_role(fonts, *role).ok_or(ERR_NOT_FOUND),
        FontKey::Named { id, acquired } => {
            if !frame.font_valid(id.namespace(), *acquired) {
                return Err(ERR_STATE);
            }
            fonts.get(id.as_str()).ok_or(ERR_STATE)
        }
        FontKey::Data(_) => Err(ERR_STATE),
    }
}

fn with_font<R>(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    call: impl FnOnce(&FontHandle<'_>) -> Result<R, i32>,
) -> Result<R, i32> {
    scope.with_frame(|frame| {
        owner.with_font(handle, |key| {
            if let FontKey::Data(font) = key {
                // Once the file is changed or removed the handle is stale.
                font.check(owner, frame.callback_epoch().ok())?;
            }
            call(&resolve(frame, key)?)
        })
    })?
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

/// The key of a font file in the plugin's own data folder: `data:<path>`.
const DATA_KEY: &str = "data:";

/// A role key, or a registered font id.
fn named_or_role(frame: &dyn Frame, key: &str) -> Result<FontKey, i32> {
    let fonts = frame.fonts().ok_or(ERR_STATE)?;
    if key.starts_with("role:") {
        let role = role(key).ok_or(ERR_NOT_FOUND)?;
        if !matches!(role, FontRole::Icons) && resolve_role(fonts, role).is_none() {
            return Err(ERR_NOT_FOUND);
        }
        return Ok(FontKey::Role(role));
    }
    let acquired = frame.font_generation().ok_or(ERR_STATE)?;
    let font = fonts.get(key).ok_or(ERR_NOT_FOUND)?;
    let id = font.id().try_clone().ok_or(ERR_NO_MEMORY)?;
    Ok(FontKey::Named { id, acquired })
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
        let key = if let Some(path) = key.strip_prefix(DATA_KEY) {
            FontKey::Data(DataFont::open(scope, owner, path)?)
        } else {
            scope.with_frame(|frame| named_or_role(frame, key))??
        };
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

/// The longest icon name in bytes.
const ICON_NAME_MAX: usize = 64;

/// Whether `name` is one `[a-z0-9-]` segment of 1 to 64 bytes.
fn icon_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= ICON_NAME_MAX
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The UTF-8 of the character of icon `name` in the bound icon font, into
/// `bytes`; the length. `ERR_INVALID` for a bad name, `ERR_NOT_FOUND` for a
/// name the host lacks or no icon font bound (or one without the character).
fn icon_utf8(frame: &dyn Frame, name: &str, bytes: &mut [u8; 4]) -> Result<usize, i32> {
    if !icon_name_ok(name) {
        return Err(ERR_INVALID);
    }
    let fonts = frame.fonts().ok_or(ERR_STATE)?;
    let font = resolve_role(fonts, FontRole::Icons).ok_or(ERR_NOT_FOUND)?;
    let ch = paperpaw_sdk::icons::char(name).ok_or(ERR_NOT_FOUND)?;
    if !font.has_char(ch) {
        return Err(ERR_NOT_FOUND);
    }
    Ok(ch.encode_utf8(bytes).len())
}

/// Slot 40: `icon.glyph`, the character of a named icon in the bound icon
/// font (`screens-input.md`): its UTF-8 (at most 4 bytes) into `out`, and
/// its length; a smaller `out` is `ERR_NO_SPACE`.
///
/// # Safety
/// `name` is live and readable, `out` writable for its capacity, disjoint
/// from each other and from host state.
pub(crate) unsafe fn icon_glyph(scope: &EnvironmentScope, name: PpBytes, out: PpBuffer) -> i32 {
    (|| {
        // SAFETY: the caller provides live readable input.
        let name = unsafe { buffers::text_in(name, ICON_NAME_MAX) }?;
        // SAFETY: the caller provides live writable output.
        let out = unsafe { buffers::buffer_out(out, ICON_NAME_MAX) }?;
        let mut bytes = [0_u8; 4];
        let len = scope.with_frame(|frame| icon_utf8(frame, name, &mut bytes))??;
        if out.len() < len {
            return Err(ERR_NO_SPACE);
        }
        for (slot, byte) in out.iter_mut().zip(&bytes[..len]) {
            slot.write(*byte);
        }
        // The length is a positive result, not a status.
        i32::try_from(len).map_err(|_| ERR_INVALID)
    })()
    .unwrap_or_else(|error| error)
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
fn ink_of(font: &FontHandle<'_>, size: f32, text: &str, threshold: u8) -> Result<[i32; 4], i32> {
    let bounds = font.ink_drawn(size, text, threshold).ok_or(ERR_NOT_FOUND)?;
    Ok([bounds.top, bounds.bottom, bounds.left, bounds.right])
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
        let values = scope.with_frame(|frame| {
            owner.with_font(handle, |key| {
                let font = resolve(frame, key)?;
                ink_of(&font, size, text, frame.ink_threshold()?)
            })
        })??;
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
                let width =
                    graphics::text_draw(canvas, font, size, x, i64::from(baseline), text, color)
                        .map_err(|_| ERR_INVALID)?;
                // Told as text where somebody asks, as a legacy label is
                // (`screens-input.md` 6.1); nothing is done otherwise.
                if canvas.is_describing() {
                    let line = font.line_metrics(size);
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a baseline on a screen is far below 2^24"
                    )]
                    let top = baseline as f32 - line.ascent;
                    let height = line.ascent + line.descent;
                    super::canvas_describe::told_text(canvas, text, x, top, width, height);
                }
                Ok(width)
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

/// The longest text slot 39 reads (`screens-input.md` section 4).
const COVERS_MAX: usize = 1024;

/// The byte length of the longest start of `text` whose characters `font`
/// draws, its fallbacks included (`FontHandle::has_char`).
fn covered(font: &FontHandle<'_>, text: &str) -> usize {
    text.char_indices()
        .find(|(_, ch)| !font.has_char(*ch))
        .map_or(text.len(), |(at, _)| at)
}

/// Slot 39: how much of `text` the font draws. The result is the length
/// itself (at most 1,024); `ERR_INVALID` above that, `ERR_UTF8` when it is no
/// text, `ERR_STATE` for a stale handle, 0 where no font is bound to the role
/// (the icon font). Case is not folded and nothing is written or allocated.
///
/// # Safety
/// `text` is readable and stable for the call.
pub(crate) unsafe fn covers(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    handle: u32,
    text: PpBytes,
) -> i32 {
    // SAFETY: the caller provides live readable input.
    let text = match unsafe { buffers::text_in(text, COVERS_MAX) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    match with_font(scope, owner, handle, |font| Ok(covered(font, text))) {
        // At most 1,024 by the bound above.
        Ok(length) => i32::try_from(length).unwrap_or(ERR_INVALID),
        Err(ERR_NOT_FOUND) => 0,
        Err(error) => error,
    }
}
