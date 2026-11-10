//! Callback-bound font handles and allocation-free text measurement.
use super::{Error, Host, binding::status, canvas::Canvas, wire::color_wire::color_encode};
use crate::Color;

/// Maximum UTF-8 key size accepted by the font.acquire slot.
pub(super) const FONT_ACQUIRE_MAX_BYTES: usize = 128;
/// Maximum UTF-8 text size accepted by the text.measure slot.
pub(super) const TEXT_MEASURE_MAX_BYTES: usize = 4096;
/// Maximum UTF-8 text size accepted by the text.draw slot.
pub(super) const TEXT_DRAW_MAX_BYTES: usize = 4096;
/// Maximum UTF-8 text size accepted by the text.ink slot.
pub(super) const TEXT_INK_MAX_BYTES: usize = 4096;
/// The longest text `Font::covers` asks about (`text.covers`).
pub(super) const TEXT_COVERS_MAX_BYTES: usize = 1024;

/// Horizontal text position and baseline in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TextOrigin {
    /// Horizontal position; fractional pixels are preserved.
    pub x: f32,
    /// Baseline row.
    pub baseline: i32,
}
impl TextOrigin {
    /// Place text at a horizontal position and baseline row.
    #[must_use]
    pub const fn new(x: f32, baseline: i32) -> Self {
        Self { x, baseline }
    }
}
use paperpaw_native::host_glue::PpBytes;

/// A reader-selected font role, resolved by the host for each text call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontRole {
    /// The user interface font.
    Ui,
    /// The bold user interface font.
    UiBold,
    /// The reader's text font.
    Reading,
    /// The reader's bold text font; the reading font where none is bound.
    ReadingBold,
    /// The reader's italic text font; the reading font where none is bound.
    ReadingItalic,
    /// The reader's bold italic text font; the bold reading font where none is bound.
    ReadingBoldItalic,
    /// The status line's font.
    Status,
    /// The monospace font.
    Mono,
}

impl FontRole {
    const fn font_role_key(self) -> &'static str {
        match self {
            Self::Ui => "role:ui",
            Self::UiBold => "role:ui-bold",
            Self::Reading => "role:reading",
            Self::ReadingBold => "role:reading-bold",
            Self::ReadingItalic => "role:reading-italic",
            Self::ReadingBoldItalic => "role:reading-bold-italic",
            Self::Status => "role:status",
            Self::Mono => "role:mono",
        }
    }
}

/// An owned font handle confined to one callback's host binding.
/// A removed or replaced font returns State from text operations; acquire
/// it again when the screen is shown. Drop releases the handle once.
///
/// ```compile_fail
/// use paperpaw_sdk::native::fonts::Font;
/// fn escape<'cb>(font: Font<'cb>) -> Font<'static> { font }
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::fonts::Font;
/// fn send<T: Send>() {}
/// send::<Font<'static>>();
/// ```
#[derive(Debug)]
pub struct Font<'cb> {
    host: &'cb Host<'cb>,
    handle: Option<u32>,
}

impl<'cb> Font<'cb> {
    /// Acquire a registered font id or a role key without allocating.
    /// The host enforces the font family's handle limit, shared by role and id
    /// handles. See `docs/abi/screens-input.md` sections 4 and 7.
    ///
    /// # Errors
    /// Returns Invalid for a key over 128 UTF-8 bytes, or the host refusal.
    pub fn acquire(host: &'cb Host<'cb>, key: impl AsRef<str>) -> Result<Self, Error> {
        let key = key.as_ref();
        if key.len() > FONT_ACQUIRE_MAX_BYTES {
            return Err(Error::Invalid);
        }
        let mut handle = 0;
        // SAFETY: the callback retains the host; key and output are call-local.
        status(unsafe { (host.table.font_acquire)(host.ctx, bytes(key), &mut handle) })?;
        Ok(Self {
            host,
            handle: Some(handle),
        })
    }

    /// Acquire a font that follows the reader's selected role on each call.
    ///
    /// # Errors
    /// Returns the host refusal, including Full and NotFound.
    pub fn acquire_role(host: &'cb Host<'cb>, role: FontRole) -> Result<Self, Error> {
        Self::acquire(host, role.font_role_key())
    }

    /// Measure text advance in pixels at a finite, positive font size.
    /// No temporary text or output buffer is allocated.
    ///
    /// # Errors
    /// Returns Invalid for a nonpositive or nonfinite size or text over 4096
    /// UTF-8 bytes, or the host refusal, including a stale handle.
    pub fn measure_text(&self, size: f32, text: impl AsRef<str>) -> Result<f32, Error> {
        let text = text.as_ref();
        validate_text(size, text, TEXT_MEASURE_MAX_BYTES)?;
        let handle = self.handle.ok_or(Error::State)?;
        let mut out = 0.0;
        // SAFETY: this font retains the callback host and its live opaque handle;
        // text and the disjoint scalar output remain live throughout the call.
        status(unsafe {
            (self.host.table.text_measure)(self.host.ctx, handle, size, bytes(text), &mut out)
        })?;
        Ok(out)
    }

    /// Draw text inside this callback's frame clip and return its advance.
    /// An error after drawing begins can leave pixels changed.
    ///
    /// # Errors
    /// Returns Invalid for an invalid size/position, a foreign canvas or
    /// overlong text, or the host's refusal.
    pub fn render_text(
        &self,
        canvas: &Canvas<'_>,
        size: f32,
        origin: TextOrigin,
        color: Color,
        text: impl AsRef<str>,
    ) -> Result<f32, Error> {
        let text = text.as_ref();
        validate_text(size, text, TEXT_DRAW_MAX_BYTES)?;
        if !origin.x.is_finite() {
            return Err(Error::Invalid);
        }
        let target = canvas.host();
        if self.host.ctx != target.ctx || !core::ptr::eq(self.host.table, target.table) {
            return Err(Error::Invalid);
        }
        let handle = self.handle.ok_or(Error::State)?;
        let mut advance = 0.0;
        // SAFETY: the font and canvas retain the same admitted callback host.
        // Text and the disjoint advance output remain live through this call.
        status(unsafe {
            (target.table.text_draw)(
                target.ctx,
                handle,
                size,
                origin.x,
                origin.baseline,
                color_encode(color.r, color.g, color.b),
                bytes(text),
                &mut advance,
            )
        })?;
        Ok(advance)
    }

    /// Release this handle and report the host status.
    /// A failed release is not retried by Drop; host teardown reclaims handles
    /// still owned by the plugin.
    ///
    /// # Errors
    /// Returns the host refusal without changing its status.
    pub fn release(mut self) -> Result<(), Error> {
        let handle = self.handle.take().ok_or(Error::State)?;
        // SAFETY: handle was acquired through this callback's host and is taken
        // before the call so Drop cannot release it a second time.
        status(unsafe { (self.host.table.handle_release)(self.host.ctx, handle) })
    }
}

impl Drop for Font<'_> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: the borrowed host outlives this font. Drop cannot report
            // errors; host teardown reclaims a handle if release is refused.
            unsafe {
                (self.host.table.handle_release)(self.host.ctx, handle);
            }
        }
    }
}

fn validate_text(size: f32, text: &str, max_bytes: usize) -> Result<(), Error> {
    validate_font_size(size)?;
    if text.len() > max_bytes {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn validate_font_size(size: f32) -> Result<(), Error> {
    if !size.is_finite() || size <= 0.0 {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn bytes(text: &str) -> PpBytes {
    PpBytes {
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

#[cfg(test)]
#[path = "fonts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "fonts_draw_tests.rs"]
mod drawing_tests;

#[path = "fonts_metrics.rs"]
mod metrics;
pub use metrics::InkBounds;

#[path = "fonts_icons.rs"]
mod icons;
pub use icons::IconGlyph;
