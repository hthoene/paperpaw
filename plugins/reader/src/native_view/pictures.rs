//! A page's pictures follow its text (#252): the first frame of a page draws
//! the text and keeps each picture's place empty, exactly its size, so the
//! page shows fast; the frame after draws the pictures in those places and
//! nothing moves. The pictures' boxes are asked for when the page changes
//! (a turn, a jump, a link, the book shown) and again when the screen is
//! shown while they wait (the host drops a follow-up of a screen that left
//! the top). A page laid out only in its draw (a changed setting, a turned
//! screen) and a page without pictures have one frame; a page turned before
//! its pictures were drawn never draws them.
use super::{Open, Setup, redraw};
use crate::native_draw::{Pictures, picture_boxes};
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::FrameRect,
    commands::{CommandSender, RedrawMode},
};

/// Where the page on show is in its two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    /// The pictures are drawn (or the page has none).
    Whole,
    /// The page has pictures and the host draws their boxes again in a
    /// follow-up frame: every draw before it (each band of a frame, the X4's
    /// second send) keeps their places empty.
    Pending,
}

impl Pass {
    /// What a draw puts in the pictures' places: while they are pending,
    /// nothing, except in the host's follow-up of their boxes (`follow_up`,
    /// each of its bands), which draws them; every draw after that does too.
    pub(super) fn drawing(&mut self, follow_up: bool) -> Pictures {
        match (*self, follow_up) {
            (Self::Pending, false) => Pictures::Places,
            (Self::Pending, true) | (Self::Whole, _) => {
                *self = Self::Whole;
                Pictures::Drawn
            }
        }
    }
}

/// Whether a draw is of the host's follow-up frame of the areas asked for
/// (`queue_redraw_area`): bit 0 of a screen draw's id (`screens-input.md`).
pub(super) const fn follow_up(frame: &paperpaw_sdk::native::callbacks::DrawFrame<'_>) -> bool {
    frame.id & 1 == 1
}

impl Open {
    /// Whether the page on show (laid out) has pictures.
    pub(super) fn has_pictures(&self) -> bool {
        self.page
            .as_ref()
            .is_some_and(|page| !page.images.is_empty())
    }

    /// Whether the pictures of the page on show wait for the host's
    /// follow-up frame.
    pub(super) fn pictures_pending(&self) -> bool {
        self.pictures == Pass::Pending
    }

    /// The page on show is drawn again in `mode` (it was laid out in the
    /// callback: `announce`). Where it has pictures and the host takes their
    /// boxes to draw again in a frame of their own after, the page is drawn
    /// first without them (#252); else with them.
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub(super) fn redraw_page(&mut self, host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
        redraw(host, mode)?;
        self.pictures = Pass::Whole;
        let Some(page) = self.page.as_ref().filter(|_| self.has_pictures()) else {
            return Ok(());
        };
        let Some(area) = Setup::read(host).ok().and_then(|setup| self.area(&setup)) else {
            return Ok(());
        };
        // The boxes from the top left of the screen's region, as its draw
        // has them (`draw_with`: the content box, then the picture).
        let origin = (f32::from(area.x), f32::from(area.y));
        if ask_second_pass(host, picture_boxes(page, origin)) {
            self.pictures = Pass::Pending;
        }
        Ok(())
    }
}

/// Asks the host to draw the pictures' boxes (`(x, y, width, height)` from
/// the top left of the screen's region) again in a frame of their own
/// right after the one asked for, with a partial refresh of the boxes only
/// (#252). Whether it takes every box.
fn ask_second_pass(host: &Host<'_>, mut boxes: impl Iterator<Item = (i32, i32, u32, u32)>) -> bool {
    let mut scratch = [0; 64];
    let mut sender = CommandSender::new(host, &mut scratch);
    // The host joins boxes past its eighth into the last; a refusal (an old
    // host, no room) draws the pictures with the text, where the boxes
    // already asked for are drawn again unchanged.
    boxes.all(|(x, y, width, height)| {
        sender
            .queue_redraw_area(RedrawMode::Partial, FrameRect::new(x, y, width, height))
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::Pass;
    use crate::native_draw::Pictures;

    #[test]
    fn the_text_is_drawn_first_and_the_pictures_in_the_follow_up() {
        let mut pass = Pass::Pending;
        // Every band of the first frame (and its second send): no pictures.
        for _ in 0..5 {
            assert_eq!(pass.drawing(false), Pictures::Places);
        }
        assert_eq!(pass, Pass::Pending);
        // The follow-up of the boxes, each of its bands: the pictures.
        assert_eq!(pass.drawing(true), Pictures::Drawn);
        assert_eq!(pass.drawing(true), Pictures::Drawn);
        // Any frame after: with them.
        assert_eq!(pass.drawing(false), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
    }
}
