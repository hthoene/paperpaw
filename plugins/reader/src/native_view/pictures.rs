//! A page's pictures follow its text (#252): the first frame of a page draws
//! the text and keeps each picture's place empty, exactly its size, so the
//! page shows fast; the frame after draws the pictures in those places and
//! nothing moves. The pictures' boxes are asked for when the page changes
//! (a turn, a jump, a link, the book shown): a page laid out only in its draw
//! (a changed setting, a turned screen) and a page without pictures have one
//! frame; a page turned before its pictures were drawn never draws them.
use super::{Open, Setup, redraw};
use crate::native_draw::{Pictures, picture_boxes};
use paperpaw_sdk::native::{Error, Host, commands::RedrawMode};

/// Where the page on show is in its two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    /// The pictures are drawn (or the page has none).
    Whole,
    /// The page has pictures and its text is to be drawn first.
    Text,
    /// The text is drawn and the host draws the pictures' boxes again in the
    /// next frame: that frame draws them.
    Due,
}

impl Pass {
    /// What a frame draws of the pictures: the text pass keeps their places
    /// empty and the frame after (the host's follow-up of their boxes, or any
    /// later one) draws them.
    pub(super) fn drawing(&mut self) -> Pictures {
        match self {
            Self::Text => {
                *self = Self::Due;
                Pictures::Places
            }
            Self::Due | Self::Whole => {
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
        let Some(page) = self.page.as_ref().filter(|page| !page.images.is_empty()) else {
            return Ok(());
        };
        let Some(area) = Setup::read(host).ok().and_then(|setup| self.area(&setup)) else {
            return Ok(());
        };
        let origin = (f32::from(area.x), f32::from(area.y));
        if ask_second_pass(host, picture_boxes(page, origin)) {
            self.pictures = Pass::Text;
        }
        Ok(())
    }
}

/// Asks the host to draw the pictures' boxes (`(x, y, width, height)` in the
/// screen's coordinates, as its draw has them) again in a frame of their own
/// right after the one asked for, with a partial refresh of the boxes only
/// (#252). Whether it takes every box.
fn ask_second_pass(host: &Host<'_>, boxes: impl Iterator<Item = (i32, i32, u32, u32)>) -> bool {
    // Until the host takes areas to draw again (`queue_redraw_area`, Kernel's
    // PR for #252), the pictures are drawn with the text.
    let _ = (host, boxes.count());
    false
}

#[cfg(test)]
mod tests {
    use super::Pass;
    use crate::native_draw::Pictures;

    #[test]
    fn the_text_is_drawn_first_and_the_pictures_in_the_frame_after() {
        let mut pass = Pass::Text;
        assert_eq!(pass.drawing(), Pictures::Places);
        assert_eq!(pass, Pass::Due);
        // The frame the host draws the boxes in again (or any later one): the
        // pictures.
        assert_eq!(pass.drawing(), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
        assert_eq!(pass.drawing(), Pictures::Drawn);
    }
}
