//! Callback-bound font handles and allocation-free text measurement.
use super::{Error, Host, binding::status, canvas::Canvas, wire::color_wire::color_encode};
use crate::Color;

/// Maximum UTF-8 key size accepted by the font.acquire slot.
pub(super) const FONT_ACQUIRE_MAX_BYTES: usize = 128;
/// The key prefix of a font file in the plugin's data folder.
const DATA_KEY: &str = "data:";
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

/// How `Font::draw_inked` inks text beyond `render_text`: glyphs widened to
/// the right, a tone curve of its own, and grays left out. The default is
/// what `render_text` draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Ink {
    extra: u32,
    tone: Option<[u8; 16]>,
    sharp: bool,
}
impl Ink {
    /// Draw as `render_text` does.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            extra: 0,
            tone: None,
            sharp: false,
        }
    }
    /// Widen every glyph by this many pixels to the right; the host clamps
    /// more than 16 to 16. Advances do not change, so measuring still lays the
    /// text out.
    #[must_use]
    pub const fn extra(mut self, pixels: u32) -> Self {
        self.extra = pixels;
        self
    }
    /// The opacities 0 to 255 of the coverage levels 0 to 15, in place of the
    /// font's own tone curve; level 0 is drawn as 0 whatever it says.
    #[must_use]
    pub const fn tone(mut self, alphas: [u8; 16]) -> Self {
        self.tone = Some(alphas);
        self
    }
    /// Draw the chosen curve without grays, at the screen's own black threshold.
    #[must_use]
    pub const fn sharp(mut self) -> Self {
        self.sharp = true;
        self
    }
}

/// A reader-selected font role, resolved by the host for each text call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontRole {
    /// The user interface font.
    Ui,
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
}

impl FontRole {
    const fn font_role_key(self) -> &'static str {
        match self {
            Self::Ui => "role:ui",
            Self::Reading => "role:reading",
            Self::ReadingBold => "role:reading-bold",
            Self::ReadingItalic => "role:reading-italic",
            Self::ReadingBoldItalic => "role:reading-bold-italic",
            Self::Status => "role:status",
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

    /// Acquire the font file at `path` in the plugin's own data folder
    /// (the key `data:<path>`): a font pack (`.ppfp`) or a TrueType or
    /// OpenType file the plugin keeps there, to draw text in it though no
    /// registry holds it (a font not installed yet). The host opens it when
    /// acquired, nothing is registered, and it needs no permission. It counts
    /// toward the plugin's font handles (8, shared with roles and ids). Once the
    /// file is changed or removed the handle answers State on text calls:
    /// acquire it again. Without allocating.
    ///
    /// # Errors
    /// Returns Invalid for a path that makes the key longer than 128 bytes
    /// (a path of at most 123), or the host's refusal: Invalid for a path
    /// outside the data folder (`..`, a leading `/`, a root) or a file that
    /// is no font, NotFound for a missing file, Full beyond the handle cap.
    pub fn acquire_data(host: &'cb Host<'cb>, path: impl AsRef<str>) -> Result<Self, Error> {
        let path = path.as_ref();
        let mut key = [0_u8; FONT_ACQUIRE_MAX_BYTES];
        let slot = key
            .get_mut(..DATA_KEY.len() + path.len())
            .ok_or(Error::Invalid)?;
        let (head, rest) = slot.split_at_mut(DATA_KEY.len());
        head.copy_from_slice(DATA_KEY.as_bytes());
        rest.copy_from_slice(path.as_bytes());
        // The prefix and the path are UTF-8, so is the whole.
        let key = core::str::from_utf8(slot).map_err(|_| Error::Invalid)?;
        Self::acquire(host, key)
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
        let handle = self.drawable(canvas, size, origin, text)?;
        let target = canvas.host();
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

    /// The handle to draw `text` with on `canvas`, after the checks both draws
    /// share: size, text length, a finite position, a canvas of this font's
    /// host and a handle not yet released.
    fn drawable(
        &self,
        canvas: &Canvas<'_>,
        size: f32,
        origin: TextOrigin,
        text: &str,
    ) -> Result<u32, Error> {
        validate_text(size, text, TEXT_DRAW_MAX_BYTES)?;
        if !origin.x.is_finite() {
            return Err(Error::Invalid);
        }
        let target = canvas.host();
        if self.host.ctx != target.ctx || !core::ptr::eq(self.host.table, target.table) {
            return Err(Error::Invalid);
        }
        self.handle.ok_or(Error::State)
    }

    /// Draw text as [`render_text`](Self::render_text) does, inked as `ink` says
    /// (`docs/abi/screens-input.md` section 4, slot 41), and return its advance.
    /// A refused call draws nothing; an old host answers Unsupported. A widening
    /// above 16 pixels is not refused: the host clamps it.
    ///
    /// # Errors
    /// Returns Invalid for an invalid size/position, a foreign canvas or
    /// overlong text, State outside `draw` or for a stale handle, or the host's
    /// refusal.
    pub fn draw_inked(
        &self,
        canvas: &Canvas<'_>,
        size: f32,
        origin: TextOrigin,
        color: Color,
        text: impl AsRef<str>,
        ink: Ink,
    ) -> Result<f32, Error> {
        let text = text.as_ref();
        let handle = self.drawable(canvas, size, origin, text)?;
        let target = canvas.host();
        let tone: &[u8] = match &ink.tone {
            Some(alphas) => alphas,
            None => &[],
        };
        let mut advance = 0.0;
        // SAFETY: the font and canvas retain the same admitted callback host.
        // Text, tone and the disjoint advance output remain live through this call.
        status(unsafe {
            (target.table.text_draw_inked)(
                target.ctx,
                handle,
                size,
                origin.x,
                origin.baseline,
                color_encode(color.r, color.g, color.b),
                bytes(text),
                ink.extra,
                bytes_of(tone),
                u32::from(ink.sharp),
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

fn bytes_of(input: &[u8]) -> PpBytes {
    PpBytes {
        ptr: input.as_ptr(),
        len: input.len(),
    }
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
