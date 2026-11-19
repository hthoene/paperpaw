//! The parts of the screen the screens on the stack have: what a full
//! screen gets (above the row of hints where it shows it), and the
//! coordinates of a screen's part.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;
use paperpaw_sdk::Gesture;

/// `point` of the screen as seen from the corner of `area`.
pub(crate) fn local(point: Point, area: Rect) -> Point {
    Point::new(point.x - i32::from(area.x), point.y - i32::from(area.y))
}

/// `gesture` as seen from the corner of `area`.
pub(crate) fn local_gesture(mut gesture: Gesture, area: Rect) -> Gesture {
    gesture.start = local(gesture.start, area);
    gesture.end = local(gesture.end, area);
    gesture
}

/// `region` of `area`'s coordinates in the screen's.
pub(crate) fn on_screen(region: Rect, area: Rect) -> Rect {
    Rect::new(
        region.x.saturating_add(area.x),
        region.y.saturating_add(area.y),
        region.width,
        region.height,
    )
}

impl Kernel {
    /// The part of the screen a full screen gets: above the row of hints
    /// where `hinted`, else all of it, below a status bar `band` high
    /// (`status_band.rs`).
    pub(crate) fn layout_below(&self, hinted: bool, band: u16) -> Rect {
        let panel = self.framebuffer.bounds();
        let row = if hinted {
            self.hint.height.min(panel.height / 2)
        } else {
            0
        };
        let band = band.min((panel.height - row) / 2);
        Rect::new(0, band, panel.width, panel.height - row - band)
    }

    /// The part of the screen screen `index` has: above the row of hints
    /// where it shows it. The row shows as the screen on top shows it
    /// ([`Kernel::hint_row`]); a full screen below an overlay keeps its
    /// part, the row lying over its bottom meanwhile.
    ///
    /// A list and the detail beside it ([`Kernel::panes`]) part what the
    /// detail would have alone.
    pub(crate) fn area_of(&self, index: usize) -> Rect {
        self.whole_area(index)
    }

    /// The part of the screen screen `index` has alone, as
    /// [`Kernel::area_of`] tells, no detail beside it.
    pub(crate) fn whole_area(&self, index: usize) -> Rect {
        let hinted = self.stack.get(index).is_some_and(|active| active.hinted);
        self.layout_below(hinted, self.status_band(index))
    }

    /// Makes the screens shown anew where the part of the screen they
    /// have changed (a detail showed beside a list, or went).
    pub(crate) fn lay_out_shown(&mut self) {
        let mut remade = false;
        for index in self.full_base()..self.stack.len() {
            let area = self.area_of(index);
            if area != self.stack[index].area {
                let old = self.stack[index].area;
                self.remake_screen(index, area);
                self.invalidate(old.union(&area));
                remade = true;
            }
        }
        if remade {
            self.sync_content();
            self.output.request_refresh(RefreshMode::Partial);
        }
    }

    /// The topmost full screen: it and the overlays above it show.
    pub(crate) fn full_base(&self) -> usize {
        (0..self.stack.len())
            .rev()
            .find(|&i| self.overlay(i).is_none())
            .unwrap_or(0)
    }

    /// The screen keys, buttons and fingers go to: the one on top.
    pub(crate) fn input_index(&self) -> Option<usize> {
        self.stack.len().checked_sub(1)
    }

    /// The part of the screen the screen input goes to has.
    pub(crate) fn input_area(&self) -> Rect {
        self.input_index()
            .map_or(Rect::default(), |index| self.stack[index].area)
    }

    /// Runs `f` with the screen input goes to ([`Kernel::input_index`]).
    pub(crate) fn with_input(
        &mut self,
        f: impl FnOnce(&mut crate::screen::Screen, &mut Turn<'_, '_>) -> Handled,
    ) -> Handled {
        match self.input_index() {
            Some(index) => self.with_screen(index, f),
            None => Handled::No,
        }
    }
}
