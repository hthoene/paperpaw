//! A page's pictures follow its text (#252): the first frame of a page draws
//! the text and keeps each picture's place empty, exactly its size, so the
//! page shows fast; the frame after draws the pictures in those places and
//! nothing moves. The pictures' boxes are asked for when the page changes
//! (a turn, a jump, a link, the book shown): a page laid out only in its draw
//! (a changed setting, a turned screen) and a page without pictures have one
//! frame, and so does the first page before the screen was ever drawn (its
//! place on the screen is not known yet); a page turned before its pictures
//! were drawn never draws them.
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
    /// The page has pictures and its text is to be drawn first, in a frame
    /// at this top left (screen pixels): the boxes were asked for there.
    Text((i32, i32)),
    /// The text is drawn and the host draws the pictures' boxes again in the
    /// next frame: that frame draws them.
    Due,
}

impl Pass {
    /// What a frame at `at` (its top left, screen pixels) draws of the
    /// pictures: the text pass keeps their places empty and the frame after
    /// (the host's follow-up of their boxes, or any later one) draws them. A
    /// frame elsewhere than the boxes were asked for (the bars came or went)
    /// draws them at once: the follow-up would draw other places.
    pub(super) fn drawing(&mut self, at: (i32, i32)) -> Pictures {
        match *self {
            Self::Text(asked) if asked == at => {
                *self = Self::Due;
                Pictures::Places
            }
            Self::Text(_) | Self::Due | Self::Whole => {
                *self = Self::Whole;
                Pictures::Drawn
            }
        }
    }
}

impl Open {
    /// The page on show is drawn again in `mode` (it was laid out in the
    /// callback: `announce`). Where it has pictures and the host takes their
    /// boxes to draw again in the frame after, the page is drawn first
    /// without them (#252); else with them.
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub(super) fn redraw_page(&mut self, host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
        redraw(host, mode)?;
        self.pictures = Pass::Whole;
        // Before the first frame the screen's place is not known: one pass.
        let Some(frame_at) = self.frame_at else {
            return Ok(());
        };
        let Some(page) = self.page.as_ref().filter(|page| !page.images.is_empty()) else {
            return Ok(());
        };
        let Some(area) = Setup::read(host).ok().and_then(|setup| self.area(&setup)) else {
            return Ok(());
        };
        // The boxes in screen pixels, as the draw puts them (`draw_with`).
        let origin = (
            frame_at.0 as f32 + f32::from(area.x),
            frame_at.1 as f32 + f32::from(area.y),
        );
        if ask_second_pass(host, picture_boxes(page, origin)) {
            self.pictures = Pass::Text(frame_at);
        }
        Ok(())
    }
}

/// Asks the host to draw the pictures' boxes (`(x, y, width, height)` in the
/// screen's coordinates, as its draw has them) again in a frame of their own
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
    fn the_text_is_drawn_first_and_the_pictures_in_the_frame_after() {
        let mut pass = Pass::Text((0, 40));
        assert_eq!(pass.drawing((0, 40)), Pictures::Places);
        assert_eq!(pass, Pass::Due);
        // The frame the host draws the boxes in again (or any later one): the
        // pictures.
        assert_eq!(pass.drawing((0, 40)), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
        assert_eq!(pass.drawing((0, 40)), Pictures::Drawn);
    }

    #[test]
    fn a_frame_elsewhere_than_the_boxes_were_asked_for_draws_the_pictures_at_once() {
        // The bars went: the screen starts higher than when the boxes were asked for.
        let mut pass = Pass::Text((0, 40));
        assert_eq!(pass.drawing((0, 0)), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
    }
}
