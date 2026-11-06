//! Borrowed input events and draw frames for flat native callbacks.
use super::{
    Error,
    declare::records_wire::{self as wire, Record},
};
use paperpaw_platform::Point;

/// The transition of a key in a native input callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyState {
    /// The key was pressed.
    Press,
    /// The key was released.
    Release,
    /// The held key repeated.
    Repeat,
}

/// The phase of a pointer contact in a native input callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerPhase {
    /// Contact began.
    Down,
    /// Contact moved.
    Move,
    /// Contact ended.
    Up,
    /// Contact was cancelled.
    Cancel,
}

/// A gesture recognised by the host for a native input callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputGestureKind {
    /// A short tap.
    Tap,
    /// A held tap.
    LongPress,
    /// A swipe towards the left.
    SwipeLeft,
    /// A swipe towards the right.
    SwipeRight,
    /// A swipe upwards.
    SwipeUp,
    /// A swipe downwards.
    SwipeDown,
}

/// Exactly one input event, borrowing text from its callback's input.
/// Times use the event clock's monotonic milliseconds. Positions are pixels
/// in the screen's pane, measured from its top left corner.
///
/// ```compile_fail
/// use paperpaw_sdk::native::callbacks::Input;
/// fn escape(bytes: &[u8]) -> Input<'static> { Input::decode(bytes).unwrap() }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Input<'a> {
    /// A key transition. Unknown key names are preserved.
    Key {
        now_ms: u64,
        key: &'a str,
        state: KeyState,
    },
    /// A pointer contact.
    Pointer {
        now_ms: u64,
        position: Point,
        phase: PointerPhase,
    },
    /// A recognised gesture.
    Gesture {
        now_ms: u64,
        position: Point,
        kind: InputGestureKind,
    },
    /// An action resolved by the host's input mapping.
    Action { now_ms: u64, action: &'a str },
}

impl<'a> Input<'a> {
    /// Decode exactly one input record without allocating.
    ///
    /// # Errors
    /// Returns generated framing, UTF-8 and field-cap errors. Unknown tags
    /// are Unsupported, and unknown state, phase or gesture values are Invalid.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        Ok(
            match wire::records_decode_single(bytes).map_err(Error::from_status)? {
                Record::InputKey(r) => Self::Key {
                    now_ms: r.now_ms,
                    key: r.key,
                    state: match r.state {
                        0 => KeyState::Press,
                        1 => KeyState::Release,
                        2 => KeyState::Repeat,
                        _ => return Err(Error::Invalid),
                    },
                },
                Record::InputPointer(r) => Self::Pointer {
                    now_ms: r.now_ms,
                    position: Point::new(r.x, r.y),
                    phase: match r.phase {
                        0 => PointerPhase::Down,
                        1 => PointerPhase::Move,
                        2 => PointerPhase::Up,
                        3 => PointerPhase::Cancel,
                        _ => return Err(Error::Invalid),
                    },
                },
                Record::InputGesture(r) => Self::Gesture {
                    now_ms: r.now_ms,
                    position: Point::new(r.x, r.y),
                    kind: match r.kind {
                        0 => InputGestureKind::Tap,
                        1 => InputGestureKind::LongPress,
                        2 => InputGestureKind::SwipeLeft,
                        3 => InputGestureKind::SwipeRight,
                        4 => InputGestureKind::SwipeUp,
                        5 => InputGestureKind::SwipeDown,
                        _ => return Err(Error::Invalid),
                    },
                },
                Record::InputAction(r) => Self::Action {
                    now_ms: r.now_ms,
                    action: r.action,
                },
                _ => return Err(Error::Unsupported),
            },
        )
    }
}

/// The purpose for which a native plugin draws a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrawPurpose {
    /// The plugin's screen.
    Screen,
    /// A reader page layer.
    PageLayer,
    /// An inserted content block.
    InsertedBlock,
    /// A status item.
    StatusItem,
    /// A widget.
    Widget,
    /// A purpose added by a newer host. Draw nothing and return success.
    Other(u32),
}

/// The full wire rectangle, preserving signed origins and 32-bit dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameRect {
    /// Left edge in screen pixels.
    pub x: i32,
    /// Top edge in screen pixels.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl FrameRect {
    /// Construct a rectangle without narrowing its wire coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// One draw request, borrowing purpose-specific bytes from the callback.
/// The host's canvas clips every draw call to the permitted rows.
///
/// ```compile_fail
/// use paperpaw_sdk::native::callbacks::DrawFrame;
/// fn escape(bytes: &[u8]) -> DrawFrame<'static> { DrawFrame::decode(bytes).unwrap() }
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::callbacks::DrawFrame;
/// fn requires_copy<T: Copy>() {}
/// requires_copy::<DrawFrame<'static>>();
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::callbacks::DrawFrame;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<DrawFrame<'static>>();
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct DrawFrame<'a> {
    host: Option<&'a super::Host<'a>>,
    /// What is being drawn.
    pub purpose: DrawPurpose,
    /// Target rectangle in screen pixels.
    pub rect: FrameRect,
    /// First row permitted by the host canvas clip.
    pub clip_top: i32,
    /// Exclusive last row permitted by the host canvas clip.
    pub clip_bottom: i32,
    /// Purpose-specific identifier.
    pub id: u32,
    /// Purpose-specific bytes, borrowed for this callback only.
    pub payload: &'a [u8],
}

impl PartialEq for DrawFrame<'_> {
    fn eq(&self, other: &Self) -> bool {
        (
            self.purpose,
            self.rect,
            self.clip_top,
            self.clip_bottom,
            self.id,
            self.payload,
        ) == (
            other.purpose,
            other.rect,
            other.clip_top,
            other.clip_bottom,
            other.id,
            other.payload,
        )
    }
}
impl Eq for DrawFrame<'_> {}

impl<'a> DrawFrame<'a> {
    pub(super) fn bind<'cb>(self, host: &'cb super::Host<'cb>) -> DrawFrame<'cb>
    where
        'a: 'cb,
    {
        let mut frame: DrawFrame<'cb> = self;
        frame.host = Some(host);
        frame
    }

    /// Borrow the drawing canvas installed for this frame's callback.
    /// Frames decoded directly from bytes have no drawing lease.
    ///
    /// ```compile_fail
    /// use paperpaw_sdk::native::{callbacks::DrawFrame, canvas::Canvas};
    /// fn escape<'a>(frame: &'a mut DrawFrame<'a>) -> Canvas<'static> {
    ///     frame.draw_canvas().unwrap()
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// use paperpaw_sdk::native::callbacks::DrawFrame;
    /// fn borrow_twice(frame: &mut DrawFrame<'_>) {
    ///     let first = frame.draw_canvas().unwrap();
    ///     let second = frame.draw_canvas().unwrap();
    ///     let _ = first.frame_rect();
    /// }
    /// ```
    ///
    /// # Errors
    /// Returns State for a frame without a callback drawing lease, or Invalid
    /// for a malformed clip.
    pub fn draw_canvas(&mut self) -> Result<super::canvas::Canvas<'_>, Error> {
        let host = self.host.ok_or(Error::State)?;
        let frame_bottom = i64::from(self.rect.y)
            .checked_add(i64::from(self.rect.height))
            .ok_or(Error::Invalid)?;
        let top = i64::from(self.clip_top);
        let bottom = i64::from(self.clip_bottom);
        if self.rect.width == 0
            || self.rect.height == 0
            || top >= bottom
            || top < i64::from(self.rect.y)
            || bottom > frame_bottom
        {
            return Err(Error::Invalid);
        }
        let height = bottom.checked_sub(top).ok_or(Error::Invalid)?;
        let clip = FrameRect::new(
            self.rect.x,
            self.clip_top,
            self.rect.width,
            u32::try_from(height).map_err(|_| Error::Invalid)?,
        );
        Ok(super::canvas::Canvas::from(host, self.rect, clip))
    }

    /// Decode exactly one draw.frame record without allocating.
    /// Unknown purposes are preserved so the caller can ignore them.
    ///
    /// # Errors
    /// Returns generated framing and field-cap errors, or Unsupported for a
    /// record other than draw.frame.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let Record::DrawFrame(r) =
            wire::records_decode_single(bytes).map_err(Error::from_status)?
        else {
            return Err(Error::Unsupported);
        };
        Ok(Self {
            host: None,
            purpose: match r.purpose {
                0 => DrawPurpose::Screen,
                1 => DrawPurpose::PageLayer,
                2 => DrawPurpose::InsertedBlock,
                3 => DrawPurpose::StatusItem,
                4 => DrawPurpose::Widget,
                other => DrawPurpose::Other(other),
            },
            rect: FrameRect::new(r.x, r.y, r.width, r.height),
            clip_top: r.clip_top,
            clip_bottom: r.clip_bottom,
            id: r.id,
            payload: r.payload,
        })
    }
}

#[cfg(test)]
#[path = "callbacks_tests.rs"]
mod tests;
