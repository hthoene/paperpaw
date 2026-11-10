//! Borrow a canvas only during a synchronous foreign draw callback.
//! The owner keeps this pinned slot, not a framebuffer or canvas reference.

use super::Error;
pub use crate::abi7::color_wire::{color_decode, color_encode};
use crate::abi7::graphics;
use core::{
    cell::{Cell, RefCell},
    fmt,
    marker::PhantomPinned,
    pin::Pin,
    ptr::NonNull,
};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_STATE, OK};
use paperpaw_platform::{Color, Rect};

/// Allocation-free drawing scope inside a pinned host context.
/// This single-threaded slot preserves the supplied canvas's clipping, origin
/// and semantic sink. It does not retain a canvas outside its callback.
#[non_exhaustive]
pub struct CanvasScope {
    current: RefCell<Option<NonNull<Canvas<'static>>>>,
    requested_clip: Cell<Option<Rect>>,
    _pin: PhantomPinned,
}

struct Reset<'a>(&'a CanvasScope);
impl Drop for Reset<'_> {
    fn drop(&mut self) {
        // Every operation's private borrow guard has ended before scope exit,
        // including unwinding. No user code can retain a guard or canvas view.
        *self.0.current.borrow_mut() = None;
        self.0.requested_clip.set(None);
    }
}

impl CanvasScope {
    /// Create an empty slot before pinning the containing context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: RefCell::new(None),
            requested_clip: Cell::new(None),
            _pin: PhantomPinned,
        }
    }

    /// Exclusively lend a canvas for one synchronous callback.
    /// No RefCell guard or temporary drawing-operation borrow spans `call`.
    /// Errors and unwinding clear the slot before the canvas borrow ends.
    ///
    /// The original canvas cannot be accessed while the callback is active:
    /// ```compile_fail
    /// use paperpaw_core::native_host::canvas::CanvasScope;
    /// use paperpaw_graphics::Framebuffer;
    /// use paperpaw_platform::{PixelFormat, Color};
    /// let mut frame = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
    /// let mut canvas = frame.canvas();
    /// let scope = core::pin::pin!(CanvasScope::new());
    /// scope.as_ref().scope(&mut canvas, || {
    ///     canvas.fill_rect(0, 0, 1, 1, Color::BLACK);
    /// });
    /// ```
    ///
    /// # Errors
    /// Returns Busy for nested scopes or while a drawing operation is active.
    pub fn scope<R>(
        self: Pin<&Self>,
        canvas: &mut Canvas<'_>,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        let mut current = self.current.try_borrow_mut().map_err(|_| Error::Busy)?;
        if current.is_some() {
            return Err(Error::Busy);
        }
        // Erase only the canvas's backing-storage lifetime. The exclusive
        // caller borrow lasts through call and Reset; no pointer is exposed.
        *current = Some(NonNull::from(canvas).cast::<Canvas<'static>>());
        drop(current);
        self.requested_clip.set(None);
        let _reset = Reset(self.get_ref());
        Ok(call())
    }

    /// Slot-9 scalar bridge: packed sRGB is 0x00RRGGBB, without alpha.
    /// A nonzero high byte returns ERR_INVALID before any drawing.
    /// Coordinates and extents have the exact declared ABI widths; clipping,
    /// origin and grayscale conversion retain the existing Canvas behavior.
    #[must_use]
    pub fn fill_rect_raw(&self, x: i32, y: i32, width: u32, height: u32, color: u32) -> i32 {
        let [r, g, b] = match color_decode(color) {
            Ok(channels) => channels,
            Err(error) => return error,
        };
        self.fill_rect(
            i64::from(x),
            i64::from(y),
            u64::from(width),
            u64::from(height),
            Color::rgb(r, g, b),
        )
    }

    /// Fill a signed rectangle through the existing checked graphics backend.
    /// Returns OK or ERR_INVALID for unrepresentable geometry, ERR_STATE outside
    /// a draw callback, and ERR_BUSY if the canvas is already in use.
    /// This typed host seam does not define or reinterpret wire color bits.
    #[must_use]
    pub fn fill_rect(&self, x: i64, y: i64, width: u64, height: u64, color: Color) -> i32 {
        self.with_canvas(
            |canvas| match graphics::fill_rect(canvas, x, y, width, height, color) {
                Ok(()) => OK,
                Err(_) => ERR_INVALID,
            },
        )
        .unwrap_or_else(|error| error)
    }

    /// Borrow the active canvas privately; no operation can retain its view.
    pub(crate) fn with_canvas<R>(&self, draw: impl FnOnce(&mut Canvas<'_>) -> R) -> Result<R, i32> {
        let mut current = self.current.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let pointer = current.as_mut().ok_or(ERR_STATE)?;
        // SAFETY: scope retains the exclusive canvas/backing-storage borrow.
        // RefMut excludes simultaneous operations; the view cannot escape draw.
        let canvas = unsafe { pointer.as_mut() };
        Ok(match self.requested_clip.get() {
            Some(region) => draw(&mut canvas.clipped(region)),
            None => draw(canvas),
        })
    }

    /// Whether a canvas is lent now: `ERR_STATE` outside a draw callback,
    /// `ERR_BUSY` while a drawing operation holds it. Nothing is drawn.
    pub(crate) fn present(&self) -> Result<(), i32> {
        let current = self.current.try_borrow().map_err(|_| ERR_BUSY)?;
        current.as_ref().map(|_| ()).ok_or(ERR_STATE)
    }

    /// Replace this draw call's clip, intersecting the original frame each time.
    #[must_use]
    pub fn clip(&self, x: i32, y: i32, width: u32, height: u32) -> i32 {
        let frame = match self.with_canvas_unclipped(|canvas| canvas.clip()) {
            Ok(frame) => frame,
            Err(error) => return error,
        };
        let left = i64::from(x).max(i64::from(frame.x));
        let top = i64::from(y).max(i64::from(frame.y));
        let right = (i64::from(x) + i64::from(width)).min(i64::from(frame.right()));
        let bottom = (i64::from(y) + i64::from(height)).min(i64::from(frame.bottom()));
        let region = if right <= left || bottom <= top {
            Rect::default()
        } else {
            let Ok(x) = u16::try_from(left) else {
                return ERR_INVALID;
            };
            let Ok(y) = u16::try_from(top) else {
                return ERR_INVALID;
            };
            let Ok(width) = u16::try_from(right - left) else {
                return ERR_INVALID;
            };
            let Ok(height) = u16::try_from(bottom - top) else {
                return ERR_INVALID;
            };
            Rect::new(x, y, width, height)
        };
        self.requested_clip.set(Some(region));
        OK
    }

    fn with_canvas_unclipped<R>(&self, read: impl FnOnce(&Canvas<'_>) -> R) -> Result<R, i32> {
        let current = self.current.try_borrow().map_err(|_| ERR_BUSY)?;
        let pointer = current.as_ref().ok_or(ERR_STATE)?;
        // SAFETY: scope retains the canvas; the shared guard excludes mutations.
        Ok(read(unsafe { pointer.as_ref() }))
    }

    /// Raw slot-10 line with the current frame clip and colour validation.
    #[allow(
        clippy::too_many_arguments,
        reason = "the ABI line has scalar geometry"
    )]
    #[must_use]
    pub fn draw_line(&self, x0: i32, y0: i32, x1: i32, y1: i32, width: u32, color: u32) -> i32 {
        let color = match packed_color(color) {
            Ok(color) => color,
            Err(error) => return error,
        };
        self.with_canvas(|canvas| {
            graphics::draw_line(
                canvas,
                i64::from(x0),
                i64::from(y0),
                i64::from(x1),
                i64::from(y1),
                u64::from(width),
                color,
            )
            .map_or(ERR_INVALID, |()| OK)
        })
        .unwrap_or_else(|error| error)
    }

    /// Raw slot-11 rounded fill.
    #[allow(
        clippy::too_many_arguments,
        reason = "the ABI round rectangle has scalar geometry"
    )]
    #[must_use]
    pub fn fill_round(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        radius: u32,
        color: u32,
    ) -> i32 {
        let color = match packed_color(color) {
            Ok(color) => color,
            Err(error) => return error,
        };
        self.with_canvas(|canvas| {
            graphics::fill_round(
                canvas,
                i64::from(x),
                i64::from(y),
                u64::from(width),
                u64::from(height),
                u64::from(radius),
                color,
            )
            .map_or(ERR_INVALID, |()| OK)
        })
        .unwrap_or_else(|error| error)
    }

    /// Raw slot-12 rounded stroke.
    #[allow(
        clippy::too_many_arguments,
        reason = "the ABI stroke has scalar geometry"
    )]
    #[must_use]
    pub fn stroke_round(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        radius: u32,
        thickness: u32,
        color: u32,
    ) -> i32 {
        let color = match packed_color(color) {
            Ok(color) => color,
            Err(error) => return error,
        };
        self.with_canvas(|canvas| {
            graphics::stroke_round(
                canvas,
                i64::from(x),
                i64::from(y),
                u64::from(width),
                u64::from(height),
                u64::from(thickness),
                u64::from(radius),
                color,
            )
            .map_or(ERR_INVALID, |()| OK)
        })
        .unwrap_or_else(|error| error)
    }
}
fn packed_color(color: u32) -> Result<Color, i32> {
    color_decode(color).map(|[r, g, b]| Color::rgb(r, g, b))
}

impl Default for CanvasScope {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Debug for CanvasScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanvasScope").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_operation_refuses_without_panicking() {
        let scope = CanvasScope::new();
        let _borrow = scope.current.borrow_mut();
        assert_eq!(scope.fill_rect(0, 0, 1, 1, Color::BLACK), ERR_BUSY);
    }
    #[test]
    fn clip_replaces_intersects_frame_and_resets_between_draws() {
        use paperpaw_graphics::Framebuffer;
        use paperpaw_platform::PixelFormat;
        let mut actual = Framebuffer::new(8, 8, PixelFormat::Gray8).unwrap();
        let mut expected = Framebuffer::new(8, 8, PixelFormat::Gray8).unwrap();
        actual.canvas().fill_rect(0, 0, 8, 8, Color::WHITE);
        expected.canvas().fill_rect(0, 0, 8, 8, Color::WHITE);
        let scope = core::pin::pin!(CanvasScope::new());
        {
            let mut base = actual.canvas();
            let mut canvas = base.clipped(Rect::new(1, 1, 6, 6));
            scope
                .as_ref()
                .scope(&mut canvas, || {
                    assert_eq!(scope.clip(2, 2, 2, 2), OK);
                    assert_eq!(scope.fill_rect(0, 0, 8, 8, Color::BLACK), OK);
                    assert_eq!(scope.clip(i32::MAX, i32::MAX, u32::MAX, u32::MAX), OK);
                    assert_eq!(scope.fill_rect(0, 0, 8, 8, Color::BLACK), OK);
                    assert_eq!(scope.clip(i32::MIN, i32::MIN, u32::MAX, u32::MAX), OK);
                    assert_eq!(scope.fill_rect(1, 1, 1, 1, Color::gray(90)), OK);
                })
                .unwrap();
            // New draw resets the previous (large) request to the frame clip.
            scope
                .as_ref()
                .scope(&mut canvas, || {
                    assert_eq!(scope.fill_rect(6, 6, 1, 1, Color::BLACK), OK);
                })
                .unwrap();
        }
        {
            let mut base = expected.canvas();
            let mut canvas = base.clipped(Rect::new(1, 1, 6, 6));
            canvas
                .clipped(Rect::new(2, 2, 2, 2))
                .fill_rect(0, 0, 8, 8, Color::BLACK);
            canvas.fill_rect(1, 1, 1, 1, Color::gray(90));
            canvas.fill_rect(6, 6, 1, 1, Color::BLACK);
        }
        assert_eq!(actual.view().data, expected.view().data);
        assert_eq!(scope.clip(0, 0, 8, 8), ERR_STATE);
        assert_eq!(scope.draw_line(0, 0, 1, 1, 1, 0), ERR_STATE);
        assert_eq!(scope.fill_round(0, 0, 1, 1, 1, 0), ERR_STATE);
        assert_eq!(scope.stroke_round(0, 0, 1, 1, 1, 1, 0), ERR_STATE);
    }
}
