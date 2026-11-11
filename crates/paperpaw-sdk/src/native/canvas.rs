//! Callback-bound drawing over the host's installed canvas.
use super::{Error, Host, binding::status, callbacks::FrameRect, wire::color_wire::color_encode};
use crate::{Color, Point};

/// Drawing access for one native draw callback. The host owns the canvas and
/// applies the frame's clipping; these methods forward pixel coordinates.
/// At most one canvas is borrowed from a draw callback at once.
///
/// ```compile_fail
/// use paperpaw_sdk::native::canvas::Canvas;
/// fn escape<'cb>(canvas: Canvas<'cb>) -> Canvas<'static> { canvas }
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, callbacks::FrameRect, canvas::Canvas};
/// fn construct(host: &Host<'_>) {
///     let canvas = Canvas::from(host, FrameRect::new(0, 0, 100, 100), FrameRect::new(0, 0, 100, 100));
/// }
/// ```
#[derive(Debug)]
pub struct Canvas<'cb> {
    host: &'cb Host<'cb>,
    frame: FrameRect,
    _clip: FrameRect,
}

impl<'cb> Canvas<'cb> {
    pub(super) fn from(host: &'cb Host<'cb>, frame: FrameRect, clip: FrameRect) -> Self {
        Self {
            host,
            frame,
            _clip: clip,
        }
    }

    pub(super) const fn host(&self) -> &'cb Host<'cb> {
        self.host
    }

    /// The target rectangle supplied with this draw callback.
    #[must_use]
    pub const fn frame_rect(&self) -> FrameRect {
        self.frame
    }

    /// Fill a rectangle in screen pixels.
    ///
    /// # Errors
    /// Returns the host refusal, including State outside a draw callback.
    pub fn fill_rectangle(&self, rect: FrameRect, color: Color) -> Result<(), Error> {
        // SAFETY: the callback host outlives this canvas and all scalar inputs.
        status(unsafe {
            (self.host.table.canvas_fill_rect)(
                self.host.ctx,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                packed(color),
            )
        })
    }

    /// Draw a line between two signed screen positions, with pixel thickness.
    ///
    /// # Errors
    /// Returns the host refusal, including State outside a draw callback.
    pub fn draw_segment(
        &self,
        start: Point,
        end: Point,
        width: u32,
        color: Color,
    ) -> Result<(), Error> {
        // SAFETY: the callback host outlives this canvas and all scalar inputs.
        status(unsafe {
            (self.host.table.canvas_draw_line)(
                self.host.ctx,
                start.x,
                start.y,
                end.x,
                end.y,
                width,
                packed(color),
            )
        })
    }

    /// Fill a rounded rectangle with a radius in pixels.
    ///
    /// # Errors
    /// Returns the host refusal, including State outside a draw callback.
    pub fn fill_rounded_rectangle(
        &self,
        rect: FrameRect,
        radius: u32,
        color: Color,
    ) -> Result<(), Error> {
        // SAFETY: the callback host outlives this canvas and all scalar inputs.
        status(unsafe {
            (self.host.table.canvas_fill_round)(
                self.host.ctx,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                radius,
                packed(color),
            )
        })
    }

    /// Stroke a rounded rectangle with radius and thickness in pixels.
    ///
    /// # Errors
    /// Returns the host refusal, including State outside a draw callback.
    pub fn stroke_rounded_rectangle(
        &self,
        rect: FrameRect,
        radius: u32,
        thickness: u32,
        color: Color,
    ) -> Result<(), Error> {
        // SAFETY: the callback host outlives this canvas and all scalar inputs.
        status(unsafe {
            (self.host.table.canvas_stroke_round)(
                self.host.ctx,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                radius,
                thickness,
                packed(color),
            )
        })
    }
}

fn packed(color: Color) -> u32 {
    color_encode(color.r, color.g, color.b)
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;

#[path = "canvas_clip.rs"]
mod clip;
pub use clip::ClipGuard;

#[path = "canvas_describe.rs"]
mod describe;
pub use describe::{PartFlags, PartRole};
