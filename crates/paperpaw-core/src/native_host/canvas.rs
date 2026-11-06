//! Borrow a canvas only during a synchronous foreign draw callback.
//! The owner keeps this pinned slot, not a framebuffer or canvas reference.

use super::Error;
use crate::abi7::graphics;
use core::{cell::RefCell, fmt, marker::PhantomPinned, pin::Pin, ptr::NonNull};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_STATE, OK};
use paperpaw_platform::Color;

/// Allocation-free drawing scope inside a pinned host context.
/// This single-threaded slot preserves the supplied canvas's clipping, origin
/// and semantic sink. It does not retain a canvas outside its callback.
#[non_exhaustive]
pub struct CanvasScope {
    current: RefCell<Option<NonNull<Canvas<'static>>>>,
    _pin: PhantomPinned,
}

struct Reset<'a>(&'a CanvasScope);
impl Drop for Reset<'_> {
    fn drop(&mut self) {
        // Every operation's private borrow guard has ended before scope exit,
        // including unwinding. No user code can retain a guard or canvas view.
        *self.0.current.borrow_mut() = None;
    }
}

impl CanvasScope {
    /// Create an empty slot before pinning the containing context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: RefCell::new(None),
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
        let _reset = Reset(self.get_ref());
        Ok(call())
    }

    /// Fill a signed rectangle through the existing checked graphics backend.
    /// Returns OK or ERR_INVALID for unrepresentable geometry, ERR_STATE outside
    /// a draw callback, and ERR_BUSY if the canvas is already in use.
    /// This typed host seam does not define or reinterpret wire color bits.
    #[must_use]
    pub fn fill_rect(&self, x: i64, y: i64, width: u64, height: u64, color: Color) -> i32 {
        let Ok(mut current) = self.current.try_borrow_mut() else {
            return ERR_BUSY;
        };
        let Some(pointer) = current.as_mut() else {
            return ERR_STATE;
        };
        // SAFETY: scope retains the exclusive caller borrow and live backing
        // storage. The RefMut excludes every overlapping draw. The reference
        // exists only for this operation, cannot escape, and is gone before
        // Reset clears the erased pointer. No callback receives this reference.
        let canvas = unsafe { pointer.as_mut() };
        match graphics::fill_rect(canvas, x, y, width, height, color) {
            Ok(()) => OK,
            Err(_) => ERR_INVALID,
        }
    }
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
}
