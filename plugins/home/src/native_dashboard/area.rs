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

use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    ui::{Instance, Ui},
};

use super::Dashboard;
use crate::native_made::Made;

impl Dashboard {
    /// Where widget `instance` was laid out, where the page stands as it was laid
    /// out and drawn (not stale, not kept under another screen, not just shown
    /// again with its books to look at): it can draw its own place alone.
    pub(crate) fn widget_place(&self, instance: Instance) -> Option<Rect> {
        if self.stale || self.fresh || self.covered || self.returned {
            return None;
        }
        self.laid
            .as_ref()?
            .cells
            .iter()
            .find_map(|cell| match &cell.widget {
                Some(Made::Host(widget)) if widget.instance() == instance => Some(cell.rect),
                _ => None,
            })
    }

    /// The minute came: the places of the page's own clock, date and status
    /// line that show something new ([`crate::native_widgets::Mine::ticked`]),
    /// to be drawn alone. None where the page is not as it was laid out and
    /// drawn: it is drawn whole when it shows.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn ticked(&self, host: &Host<'_>, ui: &Ui) -> Result<Vec<FrameRect>, Error> {
        let mut areas = Vec::new();
        if self.stale || self.fresh || self.covered || self.returned {
            return Ok(areas);
        }
        let Some(laid) = self.laid.as_ref() else {
            return Ok(areas);
        };
        let cells = laid.cells.iter().map(|cell| (&cell.widget, cell.rect));
        let status = laid.status.iter().map(|(made, rect)| (made, *rect));
        let mine = cells
            .filter_map(|(made, rect)| made.as_ref().map(|made| (made, rect)))
            .chain(status)
            .filter_map(|(made, rect)| match made {
                Made::Mine(mine) => Some((mine, rect)),
                Made::Host(_) => None,
            });
        for (widget, rect) in mine {
            if widget.ticked(host, ui) {
                paperpaw_ui_kit::mem::push(&mut areas, focus_area(rect, 0, self.origin))?;
            }
        }
        // The footer's status items, where they show (not while editing).
        if !self.editing
            && laid.footer_status.ticked(host, ui)
            && let Some(look) = laid.look.borrow().first()
        {
            let area = laid.footer_status_area(&look.style);
            paperpaw_ui_kit::mem::push(&mut areas, focus_area(area, 0, self.origin))?;
        }
        Ok(areas)
    }

    /// The part of the screen's region a widget at `rect` takes.
    pub(crate) fn widget_area(&self, rect: Rect) -> FrameRect {
        focus_area(rect, 0, self.origin)
    }
}
