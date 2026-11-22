//! The slider rows of an item list: where a track is, the number it shows, and
//! drawing its row alone while a knob moves.
use super::*;

/// The number a slider row shows and its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Track {
    /// The number the knob is at.
    pub value: i64,
    /// The smallest and the largest number.
    pub range: (i64, i64),
}

/// The − and + of a stepper row of the last frame, its index, and the track of
/// its slider.
pub(super) type Stepped = (Rect, Rect, usize, Option<Rect>);

impl Item {
    /// This row with a slider under it showing `track` (a stepper's buttons come
    /// with it); as it was where the item is a heading.
    ///
    /// # Errors
    /// [`Error::NoMemory`] only.
    pub fn with_track(mut self, track: Track) -> Result<Self, Error> {
        if let Self::Row(row) = &mut self {
            row.extra_mut()?.track = Some(track);
            row.stepper = true;
        }
        Ok(self)
    }
}

impl ItemList {
    /// The slider row whose track is at `position` in the last frame, and the
    /// track; `None` where that row is faint or no longer there.
    #[must_use]
    pub fn track_at(&self, position: Point) -> Option<(usize, Rect)> {
        self.steps
            .iter()
            .filter(|step| self.choosable(step.2))
            .find_map(|&(_, _, item, track)| {
                track.filter(|t| t.contains(position)).map(|t| (item, t))
            })
    }

    /// Replaces the value text and the number of the slider row at `index`, in
    /// place (the knob a finger moves). `false` where `index` is no slider row.
    pub fn set_number(&mut self, index: usize, value: String, number: i64) -> bool {
        match self.items.get_mut(index) {
            Some(Item::Row(row)) if row.track().is_some() => {
                row.value = value;
                if let Some(track) = row.extra.as_deref_mut().and_then(|e| e[0].track.as_mut()) {
                    track.value = number;
                }
                true
            }
            _ => false,
        }
    }

    /// Asks for row `index` alone to be drawn again, quickly (a knob that
    /// moves); the whole screen where it was not on the page drawn last.
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub fn redraw_row_fast(&self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let mut scratch = [0; 128];
        let mut to = CommandSender::new(host, &mut scratch);
        let Some(rect) = self.rect_of(index) else {
            return to.queue_redraw(RedrawMode::Fast);
        };
        let area = FrameRect::new(
            i32::from(rect.x).saturating_sub(self.origin.0),
            i32::from(rect.y).saturating_sub(self.origin.1),
            u32::from(rect.width),
            u32::from(rect.height),
        );
        to.queue_redraw_area(RedrawMode::Fast, area)
    }
}
