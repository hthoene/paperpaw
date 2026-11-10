//! Slot 35 (`ui.compose`, `docs/abi/ui.md` section 5.1): one plugin's drawing
//! inside another's.
//!
//! This is the host's front half. It checks everything that does not depend on
//! who owns the target (`kind`, being inside the caller's own `draw`, the
//! caller's clip against the rectangle) and gives the rest to the frame's
//! [`Frame::compose`](crate::abi7::context::Frame::compose): the handle
//! table, the guard and private checks, the re-entry rules and the owner's
//! `draw`. The default of that hook is `ERR_UNSUPPORTED`, which is also the
//! X4's answer until its stack gate passes (section 5.1).
//!
//! Memory: nothing is allocated here. The request is a few words on the
//! stack; the child's `draw.frame` (about 60 bytes encoded) is built by
//! [`ComposeRequest::draw_frame`] and encoded by the hook into its own
//! buffer.
//!
//! State: the child gets a *view* of the caller's canvas clipped to the
//! visible part of the rectangle ([`Canvas::clipped`] borrows, it does not
//! change the canvas), and its own canvas scope keeps its own requested clip.
//! The caller's clip and canvas are therefore what they were when the child
//! returns, whatever the child did or whether it failed.
use super::frames::Purpose;
use super::registrations::DrawFrame;
use super::shell::Context;
use crate::abi7::context::{ComposeKind, ComposeRequest};
use core::ffi::c_void;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Rect;

impl ComposeKind {
    /// The wire `kind`, or `None` for one the host does not know.
    pub(crate) const fn from_wire(kind: u32) -> Option<Self> {
        match kind {
            0 => Some(Self::Widget),
            1 => Some(Self::StatusItem),
            2 => Some(Self::StatusLine),
            _ => None,
        }
    }

    /// The purpose of the child's draw frame.
    #[allow(
        dead_code,
        reason = "the frame's compose hook (Core, H3 activation) reads it"
    )]
    pub(crate) const fn purpose(self) -> Purpose {
        match self {
            Self::Widget => Purpose::Widget,
            Self::StatusItem => Purpose::Status,
            Self::StatusLine => Purpose::StatusLine,
        }
    }
}

impl ComposeRequest {
    /// The child's `draw.frame`: the rectangle given, the visible rows as its
    /// clip, the handle as `id`. `None` when nothing is visible (the hook
    /// makes no call, section 5.1).
    #[allow(
        dead_code,
        reason = "the frame's compose hook (Core, H3 activation) reads it"
    )]
    pub(crate) fn draw_frame(&self) -> Option<DrawFrame<'static>> {
        let visible = self.visible?;
        let top = i32::from(visible.y);
        let height = i32::from(visible.height);
        Some(DrawFrame {
            purpose: self.kind.purpose().wire(),
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            clip_top: top,
            clip_bottom: top + height,
            id: self.handle,
            payload: &[],
        })
    }
}

/// The part of `(x, y, width, height)` inside `clip`; `None` if empty.
/// Computed wide, so no coordinate or size can overflow.
pub(crate) fn visible(clip: Rect, x: i32, y: i32, width: u32, height: u32) -> Option<Rect> {
    let left = i64::from(x).max(i64::from(clip.x));
    let top = i64::from(y).max(i64::from(clip.y));
    let right = (i64::from(x) + i64::from(width)).min(i64::from(clip.right()));
    let bottom = (i64::from(y) + i64::from(height)).min(i64::from(clip.bottom()));
    if right <= left || bottom <= top {
        return None;
    }
    // Inside `clip`, so inside u16; a failure is an empty result, never a panic.
    Some(Rect::new(
        u16::try_from(left).ok()?,
        u16::try_from(top).ok()?,
        u16::try_from(right - left).ok()?,
        u16::try_from(bottom - top).ok()?,
    ))
}

/// Slot 35.
///
/// # Safety
/// `ctx` is null or the pinned `Context<S>` given to `make`; the other
/// arguments are scalars.
pub(super) unsafe extern "C" fn compose<S: 'static>(
    ctx: *mut c_void,
    kind: u32,
    handle: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> i32 {
    // The plugin waits for the host, and for the target's own `draw`, from
    // here to the return (the Power-hold escape must not blame the caller).
    let _slot = super::host_slot::enter();
    // SAFETY: a nonnull context is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return ERR_INVALID;
    };
    let Some(kind) = ComposeKind::from_wire(kind) else {
        return ERR_INVALID;
    };
    // Only inside the caller's own draw callback: that is when the canvas
    // scope holds a canvas. Outside it, or while another drawing operation
    // holds the canvas (the child itself), this is ERR_STATE or ERR_BUSY.
    let result = context.canvas.with_canvas(|canvas| {
        let request = ComposeRequest {
            kind,
            handle,
            x,
            y,
            width,
            height,
            visible: visible(canvas.clip(), x, y, width, height),
        };
        let region = request.visible.unwrap_or_default();
        let mut child = canvas.clipped(region);
        run(context, &request, &mut child)
    });
    match result {
        Ok(Ok(())) => OK,
        Ok(Err(status)) | Err(status) => status,
    }
}

fn run<S: 'static>(
    context: &Context<S>,
    request: &ComposeRequest,
    canvas: &mut Canvas<'_>,
) -> Result<(), i32> {
    context
        .environment
        .with_frame(|frame| frame.compose(context.plugin_id(), request, canvas))
        .and_then(core::convert::identity)
}

#[cfg(test)]
#[path = "ui_compose_tests.rs"]
mod tests;
