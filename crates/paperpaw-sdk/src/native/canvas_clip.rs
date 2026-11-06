//! Bounded clip guards over the draw callback's installed canvas.
use super::{Canvas, Error, FrameRect, status};
use core::ops::{Deref, DerefMut};

impl<'cb> Canvas<'cb> {
    /// Restrict drawing to the intersection of this clip and `rect`.
    /// The guard restores the previous clip when it is dropped. Empty
    /// intersections are allowed and leave drawing clipped to no pixels.
    ///
    /// # Errors
    /// Returns an intersection validation error or the host refusal. A refused
    /// call leaves the current clip unchanged.
    pub fn clipped(&mut self, rect: FrameRect) -> Result<ClipGuard<'_, 'cb>, Error> {
        let previous = self._clip;
        let child = intersect_frame_clips(previous, rect)?;
        replace_clip(self, child)?;
        self._clip = child;
        Ok(ClipGuard {
            canvas: self,
            previous,
            restored: false,
        })
    }
}

/// An exclusive borrow of a canvas with a temporarily restricted clip.
/// Guards may nest; each restores its parent's clip in reverse order.
/// A host failure during Drop is recovered by the callback's frame reset.
/// Call [`restore`](Self::restore) to receive a restoration error directly.
///
/// ```compile_fail
/// use paperpaw_sdk::native::canvas::{Canvas, ClipGuard};
/// fn escape<'a, 'cb>(canvas: &'a mut Canvas<'cb>) -> ClipGuard<'static, 'cb> {
///     canvas.clipped(canvas.frame_rect()).unwrap()
/// }
/// ```
#[derive(Debug)]
pub struct ClipGuard<'a, 'cb> {
    canvas: &'a mut Canvas<'cb>,
    previous: FrameRect,
    restored: bool,
}

impl ClipGuard<'_, '_> {
    /// Restore the parent clip and report the host's status.
    /// This consumes the guard. A failed restoration is not retried by Drop,
    /// and the local clip stays at the last clip accepted by the host.
    ///
    /// # Errors
    /// Returns the host refusal. The host resets drawing state when the
    /// enclosing draw callback returns.
    pub fn restore(mut self) -> Result<(), Error> {
        self.restore_once()
    }

    fn restore_once(&mut self) -> Result<(), Error> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        replace_clip(self.canvas, self.previous)?;
        self.canvas._clip = self.previous;
        Ok(())
    }
}

impl<'cb> Deref for ClipGuard<'_, 'cb> {
    type Target = Canvas<'cb>;
    fn deref(&self) -> &Self::Target {
        self.canvas
    }
}

impl DerefMut for ClipGuard<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.canvas
    }
}

impl Drop for ClipGuard<'_, '_> {
    fn drop(&mut self) {
        if let Err(_error) = self.restore_once() {
            // Drop cannot return a status. The host resets the canvas clip at
            // callback return; keep the last accepted clip locally until then.
        }
    }
}

fn replace_clip(canvas: &Canvas<'_>, rect: FrameRect) -> Result<(), Error> {
    let host = canvas.host;
    // SAFETY: the canvas retains its callback host and exclusive drawing lease;
    // all geometry is passed as scalar wire values without borrowing buffers.
    status(unsafe { (host.table.canvas_clip)(host.ctx, rect.x, rect.y, rect.width, rect.height) })
}

fn intersect_frame_clips(parent: FrameRect, requested: FrameRect) -> Result<FrameRect, Error> {
    let right = |rect: FrameRect| {
        i64::from(rect.x)
            .checked_add(i64::from(rect.width))
            .ok_or(Error::Invalid)
    };
    let bottom = |rect: FrameRect| {
        i64::from(rect.y)
            .checked_add(i64::from(rect.height))
            .ok_or(Error::Invalid)
    };
    let left = i64::from(parent.x).max(i64::from(requested.x));
    let top = i64::from(parent.y).max(i64::from(requested.y));
    let right = right(parent)?.min(right(requested)?);
    let bottom = bottom(parent)?.min(bottom(requested)?);
    let width = right.checked_sub(left).ok_or(Error::Invalid)?.max(0);
    let height = bottom.checked_sub(top).ok_or(Error::Invalid)?.max(0);
    Ok(FrameRect::new(
        i32::try_from(left).map_err(|_| Error::Invalid)?,
        i32::try_from(top).map_err(|_| Error::Invalid)?,
        u32::try_from(width).map_err(|_| Error::Invalid)?,
        u32::try_from(height).map_err(|_| Error::Invalid)?,
    ))
}

#[cfg(test)]
#[path = "canvas_clip_tests.rs"]
mod tests;
