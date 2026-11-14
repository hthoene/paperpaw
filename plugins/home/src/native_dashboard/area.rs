//! Where a focused target is, for the frame the kernel redraws.

use paperpaw_sdk::{Rect, native::callbacks::FrameRect};

/// Where target `rect` and its ring (`room` round it) are, from the top left
/// of the screen's region at `origin`.
pub(super) fn focus_area(rect: Rect, room: i32, origin: (i32, i32)) -> FrameRect {
    let grown = |size: u16| {
        u32::from(size).saturating_add(u32::try_from(room).unwrap_or(0).saturating_mul(2))
    };
    FrameRect::new(
        i32::from(rect.x)
            .saturating_sub(room)
            .saturating_sub(origin.0),
        i32::from(rect.y)
            .saturating_sub(room)
            .saturating_sub(origin.1),
        grown(rect.width),
        grown(rect.height),
    )
}
