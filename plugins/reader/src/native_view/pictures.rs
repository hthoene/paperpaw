//! A page's pictures follow its text (#252): the first frame of a page draws
//! the text and keeps each picture's place empty, exactly its size, so the
//! page shows fast; the frame after draws the pictures in those places and
//! nothing moves. A page without pictures has one frame; a page turned before
//! its pictures were drawn never draws them.
use crate::native_draw::Pictures;
use paperpaw_sdk::layout::Page;

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
    /// The first pass of a page laid out anew.
    pub(super) fn of(page: &Page) -> Self {
        if page.images.is_empty() {
            Self::Whole
        } else {
            Self::Text
        }
    }

    /// What a frame draws of the pictures: in the text pass the pictures'
    /// places stay empty where the host takes the boxes to draw again
    /// (`ask`); where it does not, they are drawn at once.
    pub(super) fn drawing(&mut self, ask: impl FnOnce() -> bool) -> Pictures {
        match self {
            Self::Text if ask() => {
                *self = Self::Due;
                Pictures::Places
            }
            Self::Text | Self::Due | Self::Whole => {
                *self = Self::Whole;
                Pictures::Drawn
            }
        }
    }
}

/// Asks the host to draw the pictures' boxes (`(x, y, width, height)` in the
/// draw's coordinates) again in a frame of their own, right after this one,
/// with a partial refresh of the boxes only (#252). Whether it will.
pub(super) fn ask_second_pass(
    host: &paperpaw_sdk::native::Host<'_>,
    boxes: impl Iterator<Item = (i32, i32, u32, u32)>,
) -> bool {
    // Until the host takes areas to draw again (`queue_redraw_area`, Kernel's
    // PR for #252), the pictures are drawn with the text.
    let _ = (host, boxes);
    false
}

#[cfg(test)]
mod tests {
    use super::Pass;
    use crate::native_draw::Pictures;

    #[test]
    fn the_text_is_drawn_first_and_the_pictures_in_the_frame_after() {
        let mut pass = Pass::Text;
        assert_eq!(pass.drawing(|| true), Pictures::Places);
        assert_eq!(pass, Pass::Due);
        // The frame the host draws the boxes in again: the pictures.
        assert_eq!(pass.drawing(|| unreachable!()), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
        assert_eq!(pass.drawing(|| unreachable!()), Pictures::Drawn);
    }

    #[test]
    fn a_host_that_does_not_draw_again_gets_the_pictures_at_once() {
        let mut pass = Pass::Text;
        assert_eq!(pass.drawing(|| false), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
    }
}
