//! A list and the item chosen in it beside each other where the screen is
//! wide ([`Pane::DETAIL`]): which screens show so, the parts of the screen
//! they have, which of the two input goes to, the line between them.
//! Two screens of the stack as ever: nothing routes commands between them,
//! and where the screen is not wide (the X4 in either orientation) a
//! detail is an ordinary full screen.
//!
//! Built with the `panes` feature (by default): devices that are never
//! wide (the Xteink ones) build without it, and save its code; there no
//! screen shows beside another and [`actions::PANE_OTHER`] is not there.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;
use paperpaw_sdk::Pane;
use paperpaw_sdk::ui::ScreenClass;

/// The list's part is at least this many millimetres wide …
const LIST_MM: u32 = 60;

/// … and at least this share of the screen, in percent; never more than
/// half of it.
const LIST_PERCENT: u32 = 38;

/// The line between the two parts, in tenths of a millimetre.
const LINE_TENTHS: u32 = 2;

/// The action that moves input to the other part, list or detail; none
/// without the `panes` feature.
pub(crate) fn register(r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
    use paperpaw_sdk::FixedAction;
    use paperpaw_sdk::builtin::actions;
    static PANES: [FixedAction; 1] =
        [
            FixedAction::new(actions::PANE_OTHER, "core.action.pane-other").handler(|_, _, out| {
                out.push(Command::extended(actions::PANE_OTHER, Params::new()));
            }),
        ];
    if cfg!(feature = "panes") {
        r.add_actions(&PANES)?;
    }
    Ok(())
}

impl Kernel {
    /// The topmost full screen: it and the overlays above it show (and the
    /// list beside it, see [`Kernel::panes`]).
    pub(crate) fn full_base(&self) -> usize {
        (0..self.stack.len())
            .rev()
            .find(|&i| self.overlay(i).is_none())
            .unwrap_or(0)
    }

    /// The list and the item chosen in it where they show beside each
    /// other, by their places on the stack: the topmost full screen is a
    /// [`Pane::DETAIL`], the screen right below it a full screen that is
    /// not, neither has all of the panel, no screen guards the device, and
    /// the part the detail would have alone is
    /// [`ScreenClass::Expanded`].
    pub(crate) fn panes(&self) -> Option<(usize, usize)> {
        // Most devices never have a detail open: nothing else is asked.
        if !cfg!(feature = "panes") || !self.stack.iter().any(|active| active.pane == Pane::DETAIL)
        {
            return None;
        }
        let detail = self.full_base();
        let list = detail.checked_sub(1)?;
        let beside = self.stack[detail].pane == Pane::DETAIL
            && self.stack[list].pane != Pane::DETAIL
            && self.guard().is_none()
            && self.overlay(list).is_none()
            && !self.full_screen(list)
            && !self.full_screen(detail);
        (beside && self.wide(self.whole_area(detail))).then_some((list, detail))
    }

    /// Whether `area` has room for a list and a detail beside it.
    fn wide(&self, area: Rect) -> bool {
        ScreenClass::of(&self.shared.env(self.now), area) >= ScreenClass::Expanded
    }

    /// `whole`, the part a full screen has, parted for a list and its
    /// detail: the list's part at the start (on the left, on the right in
    /// a right-to-left interface), the line, the detail's part.
    pub(crate) fn pane_parts(&self, whole: Rect) -> (Rect, Rect, Rect) {
        let metrics = Metrics::new(self.shared.device.display.dpi);
        let width = u32::from(whole.width);
        let list = metrics
            .mm(LIST_MM)
            .max(width * LIST_PERCENT / 100)
            .min(width / 2) as u16;
        let line = (metrics.tenth_mm(LINE_TENTHS).max(1) as u16).min(whole.width - list);
        let rest = whole.width - list - line;
        let part = |x: u16, width: u16| Rect::new(x, whole.y, width, whole.height);
        let rtl = self.shared.translator.direction() == paperpaw_sdk::lang::Direction::Rtl;
        if rtl {
            let line_x = whole.x + rest;
            (
                part(line_x + line, list),
                part(line_x, line),
                part(whole.x, rest),
            )
        } else {
            let line_x = whole.x + list;
            (
                part(whole.x, list),
                part(line_x, line),
                part(line_x + line, rest),
            )
        }
    }

    /// The line between the list and its detail, where they show.
    pub(crate) fn pane_line(&self) -> Option<Rect> {
        let (_, detail) = self.panes()?;
        Some(self.pane_parts(self.whole_area(detail)).1)
    }

    /// The part of the screen screen `screen`, just made in `area` (all a
    /// full screen has), has as the detail of the list on top: where it
    /// says it is one ([`Pane::DETAIL`]), is a full screen, the screen on
    /// top a full screen that is no detail itself, and `area` is wide.
    pub(crate) fn beside_top(&self, screen: &dyn Screen, pane: Pane, area: Rect) -> Option<Rect> {
        if !cfg!(feature = "panes")
            || pane != Pane::DETAIL
            || self.guard().is_some()
            || screen.mode().is_full_screen()
        {
            return None;
        }
        let top = self.stack.len().checked_sub(1)?;
        let env = self.shared.env(self.now);
        let device = env.device_within(area);
        let below = &self.stack[top];
        let fits = screen.region(&env.within(&device, area)).is_none()
            && below.pane != Pane::DETAIL
            && self.overlay(top).is_none()
            && !self.full_screen(top)
            && self.wide(area);
        fits.then(|| self.pane_parts(area).2)
    }

    /// A detail (`pane`, a full screen where `full`) the list beside the
    /// detail on top opens, input being the list's: the detail shown goes
    /// first (told as when it closes), so that going through a list never
    /// piles details up. Whether it went.
    pub(crate) fn close_detail_for(&mut self, pane: Pane, full: bool) -> bool {
        let Some((list, detail)) = self.panes() else {
            return false;
        };
        let top = self.stack.len() - 1;
        if pane != Pane::DETAIL || !full || detail != top || self.input_index() != Some(list) {
            return false;
        }
        self.with_screen(detail, |screen, cx| {
            screen.on_close(cx);
            Handled::Yes
        });
        self.stack.truncate(detail);
        self.screens_changed = true;
        self.stop_for_closed_screens();
        true
    }

    /// The screen keys, buttons and fingers go to: the list beside the
    /// detail on top where they were moved to it, else the screen on top.
    pub(crate) fn input_index(&self) -> Option<usize> {
        let top = self.stack.len().checked_sub(1)?;
        let Some(serial) = self.input.pane_keys else {
            return Some(top);
        };
        match self.panes() {
            Some((list, detail)) if detail == top && self.stack[list].serial == serial => {
                Some(list)
            }
            _ => Some(top),
        }
    }

    /// The part of the screen the screen input goes to has.
    pub(crate) fn input_area(&self) -> Rect {
        self.input_index()
            .map_or(Rect::default(), |index| self.stack[index].area)
    }

    /// Runs `f` with the screen input goes to ([`Kernel::input_index`]).
    pub(crate) fn with_input(
        &mut self,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        match self.input_index() {
            Some(index) => self.with_screen(index, f),
            None => Handled::No,
        }
    }

    /// The list and the detail on top beside it, if so.
    fn panes_on_top(&self) -> Option<(usize, usize)> {
        self.panes()
            .filter(|&(_, detail)| detail + 1 == self.stack.len())
    }

    /// A finger came down at `point`: input goes to the part, list or
    /// detail, it came down in from now on.
    pub(crate) fn pane_touched(&mut self, point: Point) {
        let Some((list, detail)) = self.panes_on_top() else {
            return;
        };
        let (list_area, detail_area) = (self.stack[list].area, self.stack[detail].area);
        let to_list = if list_area.contains(point) {
            true
        } else if detail_area.contains(point) {
            false
        } else if list_area.union(&detail_area).contains(point) {
            // On the line between them: the nearer part's.
            let off = |area: Rect| {
                let (start, end) = (i32::from(area.x), i32::from(area.x) + i32::from(area.width));
                (start - point.x).max(point.x - end + 1).max(0)
            };
            off(list_area) <= off(detail_area)
        } else {
            return;
        };
        self.input.pane_keys = to_list.then_some(self.stack[list].serial);
    }

    /// The first screen shown as the screens' parts are now, before they
    /// are laid out for a change ([`Kernel::shown_base`] tells it after):
    /// the list where the detail on it has less than its whole part.
    pub(crate) fn shown_as_laid_out(&self) -> usize {
        let base = self.full_base();
        let beside = cfg!(feature = "panes")
            && self.stack.get(base).is_some_and(|detail| {
                detail.pane == Pane::DETAIL && detail.area != self.whole_area(base)
            });
        match base.checked_sub(1) {
            Some(list) if beside => list,
            _ => base,
        }
    }

    /// The interface's size changed, and the class of the screen with it
    /// ([`ScreenClass::of`]): where a detail now shows beside its list, or
    /// no longer does, the screens are made anew for their parts (`shown`:
    /// [`Kernel::shown_as_laid_out`] before).
    pub(crate) fn sync_panes(&mut self, shown: usize) {
        let base = self.full_base();
        let stale = cfg!(feature = "panes")
            && (base.saturating_sub(1)..=base)
                .any(|i| self.stack.get(i).is_some_and(|s| s.area != self.area_of(i)));
        if stale {
            self.remake_screens(shown);
        }
    }

    /// Input goes to the other part, list or detail
    /// ([`actions::PANE_OTHER`](paperpaw_sdk::builtin::actions::PANE_OTHER)).
    pub(crate) fn other_pane(&mut self) {
        let Some((list, _)) = self.panes_on_top() else {
            return;
        };
        let serial = self.stack[list].serial;
        self.input.pane_keys = match self.input.pane_keys {
            Some(keys) if keys == serial => None,
            _ => Some(serial),
        };
    }

    /// After the stack changed: input goes to the screen on top again
    /// where the list it went to no longer shows beside a detail.
    pub(crate) fn sync_pane_keys(&mut self) {
        let Some(serial) = self.input.pane_keys else {
            return;
        };
        if self
            .panes()
            .is_none_or(|(list, _)| self.stack[list].serial != serial)
        {
            self.input.pane_keys = None;
        }
    }

    /// The list beside the detail on top runs as the screen on top does:
    /// when it wants [`Screen::on_timer`].
    pub(crate) fn pane_list_due(&self) -> Option<Instant> {
        let (list, _) = self.panes_on_top()?;
        self.stack[list].timer
    }

    /// Tells the list beside the detail on top that its time came.
    pub(crate) fn tick_pane_list(&mut self) {
        let Some((list, _)) = self.panes_on_top() else {
            return;
        };
        if self.stack[list].timer.is_some_and(|at| at <= self.now) {
            self.stack[list].timer = None;
            self.with_screen(list, |screen, cx| {
                screen.on_timer(cx);
                Handled::Yes
            });
        }
    }

    /// The list beside the detail shows again with it ([`Screen::on_show`]).
    pub(crate) fn show_pane_list(&mut self) {
        if let Some((list, _)) = self.panes() {
            self.with_screen(list, |screen, cx| {
                screen.on_show(cx);
                Handled::Yes
            });
        }
    }
}
