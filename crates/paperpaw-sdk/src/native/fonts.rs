//! Callback-bound font handles and allocation-free text measurement.
use super::{Error, Host, binding::status};
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
    /// The monospace font.
    Mono,
}

impl FontRole {
    const fn font_role_key(self) -> &'static str {
        match self {
            Self::Ui => "role:ui",
            Self::UiBold => "role:ui-bold",
            Self::Reading => "role:reading",
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
    /// The host allows at most four live font handles per plugin.
    ///
    /// # Errors
    /// Returns NoSpace for a key over 128 UTF-8 bytes, or the host refusal.
    pub fn acquire(host: &'cb Host<'cb>, key: impl AsRef<str>) -> Result<Self, Error> {
        let key = key.as_ref();
        if key.len() > 128 {
            return Err(Error::NoSpace);
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
    /// Returns Invalid for a nonpositive or nonfinite size, NoSpace for text
    /// over 4096 UTF-8 bytes, or the host refusal, including a stale handle.
    pub fn measure_text(&self, size: f32, text: impl AsRef<str>) -> Result<f32, Error> {
        let text = text.as_ref();
        if !size.is_finite() || size <= 0.0 {
            return Err(Error::Invalid);
        }
        if text.len() > 4096 {
            return Err(Error::NoSpace);
        }
        let handle = self.handle.ok_or(Error::State)?;
        let mut out = 0.0;
        // SAFETY: this font retains the callback host and its live opaque handle;
        // text and the disjoint scalar output remain live throughout the call.
        status(unsafe {
            (self.host.table.text_measure)(self.host.ctx, handle, size, bytes(text), &mut out)
        })?;
        Ok(out)
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

fn bytes(text: &str) -> PpBytes {
    PpBytes {
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

#[cfg(test)]
#[path = "fonts_tests.rs"]
mod tests;
