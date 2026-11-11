//! A page's pictures follow its text (#252): the first frame of a page draws
//! the text and keeps each picture's place empty, exactly its size, so the
//! page shows fast; the frame after draws the pictures in those places and
//! nothing moves. A page without pictures has one frame; a page turned before
//! its pictures were drawn never draws them.
use super::{Body, View};
use crate::native_draw::{self, Pictures};
use paperpaw_sdk::layout::Page;

/// Where the page on show is in its two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    /// The pictures are drawn (or the page has none).
    Whole,
    /// The page has pictures and its text is to be drawn first.
    Text,
    /// The text is drawn; the pictures wait for the next frame.
    TextDrawn,
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

    /// What a frame draws of the pictures now; the text pass is noted drawn.
    pub(super) fn drawing(&mut self) -> Pictures {
        match self {
            Self::Whole => Pictures::Drawn,
            Self::Text | Self::TextDrawn => {
                *self = Self::TextDrawn;
                Pictures::Places
            }
        }
    }
}

impl View {
    /// Whether the page's pictures are due in a frame now: its text was drawn
    /// without them. The next frame draws them (asked once).
    pub fn pictures_due(&mut self) -> bool {
        let Body::Open(open) = &mut self.body else {
            return false;
        };
        if open.pictures != Pass::TextDrawn {
            return false;
        }
        open.pictures = Pass::Whole;
        true
    }

    /// Whether the page on show still waits to draw its pictures.
    #[must_use]
    pub fn pictures_waiting(&self) -> bool {
        matches!(&self.body, Body::Open(open) if open.pictures != Pass::Whole)
    }

    /// The boxes the pictures of the page on show take in the content box
    /// (`(x, y, width, height)`, relative to the content box's top left).
    pub fn picture_boxes(&self) -> impl Iterator<Item = (i32, i32, u32, u32)> + '_ {
        let page = match &self.body {
            Body::Open(open) => open.page.as_ref(),
            _ => None,
        };
        page.into_iter()
            .flat_map(|page| native_draw::picture_boxes(page, (0.0, 0.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::Pass;
    use crate::native_draw::Pictures;

    #[test]
    fn the_text_is_drawn_first_and_the_pictures_after() {
        let mut pass = Pass::Text;
        assert_eq!(pass.drawing(), Pictures::Places);
        assert_eq!(pass, Pass::TextDrawn);
        // A frame again before the pictures are due (a mark, the status line):
        // still without them.
        assert_eq!(pass.drawing(), Pictures::Places);
        let mut whole = Pass::Whole;
        assert_eq!(whole.drawing(), Pictures::Drawn);
        assert_eq!(whole, Pass::Whole);
    }
}
