//! Font metrics and ink queried through the callback's live host binding.
use super::{Font, bytes, validate_text};
use crate::{
    LineMetrics,
    native::{Error, binding::status},
};
use paperpaw_native::host_glue::ERR_NOT_FOUND;

/// Text ink bounds in pixels, relative to the baseline and the pen's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct InkBounds {
    /// Top row relative to the baseline; above the baseline is negative.
    pub top: i32,
    /// Bottom row relative to the baseline.
    pub bottom: i32,
    /// Left column relative to the pen's start.
    pub left: i32,
    /// Right column relative to the pen's start.
    pub right: i32,
}
impl InkBounds {
    /// Construct bounds without altering the host's pixel coordinates.
    #[must_use]
    pub const fn new(top: i32, bottom: i32, left: i32, right: i32) -> Self {
        Self {
            top,
            bottom,
            left,
            right,
        }
    }
}

impl Font<'_> {
    /// Query ascent, descent and line height in pixels at a finite, positive size.
    /// This operation works outside a draw callback and allocates nothing.
    ///
    /// # Errors
    /// Returns Invalid for a nonpositive or nonfinite size, or the host's refusal,
    /// including State when the font handle is stale.
    pub fn read_line_metrics(&self, size: f32) -> Result<LineMetrics, Error> {
        validate_text(size, "")?;
        let handle = self.handle.ok_or(Error::State)?;
        let mut ascent = 0.0;
        let mut descent = 0.0;
        let mut line_height = 0.0;
        // SAFETY: the font retains its callback host and opaque handle. The three
        // disjoint scalar outputs remain live for the duration of the call.
        status(unsafe {
            (self.host.table.text_metrics)(
                self.host.ctx,
                handle,
                size,
                &mut ascent,
                &mut descent,
                &mut line_height,
            )
        })?;
        Ok(LineMetrics::new(ascent, descent, line_height))
    }

    /// Query ink bounds at a finite, positive size without drawing or allocating.
    /// Pixels at least half covered count as ink. Text without ink returns None.
    /// This operation works outside a draw callback.
    ///
    /// # Errors
    /// Returns Invalid for a nonpositive or nonfinite size, NoSpace for overlong
    /// text, or the host's refusal, including State when the font handle is stale.
    pub fn ink(&self, size: f32, text: impl AsRef<str>) -> Result<Option<InkBounds>, Error> {
        let text = text.as_ref();
        validate_text(size, text)?;
        let handle = self.handle.ok_or(Error::State)?;
        let mut top = 0;
        let mut bottom = 0;
        let mut left = 0;
        let mut right = 0;
        // SAFETY: the font retains its callback host and opaque handle; text and
        // the four disjoint scalar outputs remain live throughout the call.
        let result = unsafe {
            (self.host.table.text_ink)(
                self.host.ctx,
                handle,
                size,
                bytes(text),
                &mut top,
                &mut bottom,
                &mut left,
                &mut right,
            )
        };
        if result == ERR_NOT_FOUND {
            return Ok(None);
        }
        status(result)?;
        Ok(Some(InkBounds::new(top, bottom, left, right)))
    }
}

#[cfg(test)]
#[path = "fonts_metrics_tests.rs"]
mod tests;
