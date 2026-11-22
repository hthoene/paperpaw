//! The room above the rows of a list's first page ([`ItemList::set_head`]),
//! which the owner fills with what it composes or draws in.
use super::{Error, Font, ItemList, Paint, Rect, Spot, Style, heading, text};
use alloc::{string::String, vec::Vec};

/// Room above the rows of the first page that the owner fills with something it
/// composes in (a widget's preview), under a heading. The list reserves it and says
/// where it is ([`ItemList::head_rect`]); it draws nothing in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Head {
    title: String,
    /// The room's height in pixels, under the heading; 0 takes no room, heading
    /// included.
    height: u16,
    /// The room gives way to all the rows, not only the first
    /// ([`ItemList::set_head_keeps_rows`]).
    keeps_rows: bool,
}

impl ItemList {
    /// Keeps room above the rows of the first page, under the heading `title`,
    /// for something the owner composes in: none until its height is set
    /// ([`Self::set_head_height`]).
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the text cannot be copied.
    pub fn set_head(&mut self, title: &str) -> Result<(), Error> {
        self.head = Some(Head {
            title: text::copy(title)?,
            height: 0,
            keeps_rows: false,
        });
        Ok(())
    }

    /// The room above the rows gives way to all of them, not only the first:
    /// they stay on its page with it, where they fit a page (a code above the
    /// few rows that go with it, drawn as large as the rest leaves).
    pub fn set_head_keeps_rows(&mut self) {
        if let Some(head) = &mut self.head {
            head.keeps_rows = true;
        }
    }

    /// Gives the room above the rows `height` pixels (0: none, no heading);
    /// whether that changed what the list shows. A room higher than the page
    /// leaves is cut at the draw, so that the first row stays on it.
    pub fn set_head_height(&mut self, height: u16) -> bool {
        match &mut self.head {
            Some(head) if head.height != height => {
                head.height = height;
                true
            }
            _ => false,
        }
    }

    /// The room above the rows as the last frame drew it, in the frame's pixels;
    /// `None` where there is none or the page shown is not the first.
    #[must_use]
    pub const fn head_rect(&self) -> Option<Rect> {
        match self.spot {
            Spot::Head(rect) => Some(rect),
            _ => None,
        }
    }

    /// The height of the head on the first page, kept to the page: the first
    /// row stays on it, or all the rows where the head keeps them and they fit.
    #[inline(never)]
    pub(super) fn head_high(&self, style: &Style, room: u16, heights: &[u16]) -> u16 {
        let Some(head) = &self.head else {
            return 0;
        };
        let first = heights.first().copied();
        let all = heights
            .iter()
            .fold(0_u16, |sum, high| sum.saturating_add(*high));
        let below = if head.keeps_rows && all <= room {
            Some(all)
        } else {
            first
        };
        head_fit(head_height(style, head), room, below)
    }

    /// Draws the head's heading in `room` (none: nothing) and gives the room under
    /// it.
    pub(super) fn draw_head(
        &self,
        body: &mut Vec<Paint>,
        (font, style): (&Font<'_>, &Style),
        room: Rect,
    ) -> Result<Option<Rect>, Error> {
        match &self.head {
            Some(head) if room.height > 0 => head_room(body, font, style, room, head),
            _ => Ok(None),
        }
    }
}

/// The height of the head with its room: none while the room has none.
fn head_height(style: &Style, head: &Head) -> u16 {
    if head.height == 0 {
        0
    } else {
        style.heading.saturating_add(head.height)
    }
}

/// The head's height kept to the page: the first row stays on it, so a head as
/// high as the page or higher is cut to the room the first row leaves.
pub(super) fn head_fit(head: u16, room: u16, first_row: Option<u16>) -> u16 {
    head.min(room.saturating_sub(first_row.unwrap_or(0)))
}

/// Draws the heading of the head and gives the room under it.
fn head_room(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    head: &Head,
) -> Result<Option<Rect>, Error> {
    let top = Rect::new(rect.x, rect.y, rect.width, style.heading.min(rect.height));
    heading(paint, font, style, top, &head.title)?;
    Ok(Some(Rect::new(
        rect.x,
        rect.y.saturating_add(top.height),
        rect.width,
        rect.height.saturating_sub(top.height),
    )))
}
