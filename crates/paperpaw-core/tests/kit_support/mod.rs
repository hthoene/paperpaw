//! What the kit's drawing tests with an allocator of their own share: a
//! frame cleared to a gray no drawing uses (#300).

use paperpaw_graphics::Framebuffer;
use paperpaw_sdk::{Color, PixelFormat, Point, Rect};

/// An odd gray no drawing uses: where a pixel is not this, something was
/// drawn.
pub const PAPER: Color = Color::gray(77);

/// A framebuffer `width` × `height` cleared to [`PAPER`].
pub fn frame(width: u16, height: u16) -> Framebuffer {
    let mut frame = Framebuffer::new(width, height, PixelFormat::Gray8).unwrap();
    frame.canvas().clear(PAPER);
    frame.take_dirty();
    frame
}

/// The first pixel drawn on (changed from [`PAPER`]) outside `bound`,
/// looked for where the drawing touched the frame since it was made.
pub fn drawn_outside(frame: &mut Framebuffer, bound: Rect) -> Option<Point> {
    let dirty = frame.take_dirty()?;
    let view = frame.view();
    for y in dirty.y..(dirty.bottom() as u16) {
        for x in dirty.x..(dirty.right() as u16) {
            let point = Point::new(i32::from(x), i32::from(y));
            if !bound.contains(point) && view.pixel(x, y) != PAPER {
                return Some(point);
            }
        }
    }
    None
}

/// Whether anything was drawn inside `area`.
pub fn drawn_in(frame: &Framebuffer, area: Rect) -> bool {
    let view = frame.view();
    (area.y..area.bottom() as u16)
        .any(|y| (area.x..area.right() as u16).any(|x| view.pixel(x, y) != PAPER))
}
