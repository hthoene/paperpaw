//! `command.redraw_area` (`docs/abi/screens-input.md` 3): a redraw of part of
//! the submitting plugin's top screen, as interface 6's
//! `Cx::request_redraw_area`. Each area is drawn again on its own, so what
//! the refresh weighs is what the areas cover, not the rectangle around
//! them: a picture box or two on a page is no page turn (`refresh.rs`).
//! Areas asked for together with the whole screen are a **follow-up**: the
//! whole screen is drawn first, then, in the frame right after it, only the
//! areas (a page's text, then its pictures).
use super::CommandStage;
use crate::abi7::commands_wire::{Command as WireCommand, CommandBatch};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_platform::{Rect, RefreshMode};
use paperpaw_sdk::ScreenKey;

/// Areas kept per callback; one more is joined into the last kept.
pub(crate) const MAX_AREAS: usize = 8;

/// An area as the plugin named it, in screen pixels (as `DrawFrame::rect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Area {
    x: i64,
    y: i64,
    right: i64,
    bottom: i64,
}

impl Area {
    /// None for an empty area: it redraws nothing.
    pub(crate) fn new(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        (width > 0 && height > 0).then(|| Self {
            x: i64::from(x),
            y: i64::from(y),
            right: i64::from(x) + i64::from(width),
            bottom: i64::from(y) + i64::from(height),
        })
    }

    fn union(self, other: Self) -> Self {
        Self {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    /// What of `region` it covers; none when it lies outside.
    pub(crate) fn within(self, region: Rect) -> Option<Rect> {
        let x = self.x.max(i64::from(region.x));
        let y = self.y.max(i64::from(region.y));
        let right = self.right.min(i64::from(region.right()));
        let bottom = self.bottom.min(i64::from(region.bottom()));
        if right <= x || bottom <= y {
            return None;
        }
        // Inside `region`, so every value fits its `u16`s.
        Some(Rect::new(
            u16::try_from(x).ok()?,
            u16::try_from(y).ok()?,
            u16::try_from(right - x).ok()?,
            u16::try_from(bottom - y).ok()?,
        ))
    }
}

/// The areas of a callback and the strongest mode they asked for.
#[derive(Debug, Default)]
pub(crate) struct Areas {
    pub(crate) mode: Option<u32>,
    pub(crate) list: Vec<Area>,
}

impl Areas {
    fn push(&mut self, mode: u32, area: Option<Area>) {
        self.mode = Some(self.mode.map_or(mode, |old| old.max(mode)));
        if let Some(area) = area {
            join(&mut self.list, area);
        }
    }
}

/// Keeps `area` in `list` (room for [`MAX_AREAS`] reserved): once; past
/// the kept ones joined into the last.
fn join(list: &mut Vec<Area>, area: Area) {
    if list.contains(&area) {
        return;
    }
    if list.len() < MAX_AREAS {
        // Reserved before the batch was accepted: no allocation.
        list.push(area);
    } else if let Some(last) = list.last_mut() {
        *last = last.union(area);
    }
}

/// `command.redraw`'s mode values: 0 asks for none.
pub(crate) fn refresh_mode(mode: u32) -> Option<RefreshMode> {
    match mode {
        1 => Some(RefreshMode::Fast),
        2 => Some(RefreshMode::Partial),
        3 => Some(RefreshMode::Full),
        _ => None,
    }
}

/// Areas to draw in the frame after the one that draws their screen whole.
#[derive(Debug)]
pub(crate) struct Followup {
    pub(crate) screen: ScreenKey,
    pub(crate) mode: Option<RefreshMode>,
    pub(crate) areas: Vec<Area>,
}

impl crate::Kernel {
    /// Draws each of `areas` again, as much of it as the screen on top has,
    /// and asks for `mode`.
    pub(crate) fn redraw_areas(&mut self, areas: &[Area], mode: Option<RefreshMode>) {
        let region = self
            .stack
            .len()
            .checked_sub(1)
            .map_or(self.content(), |top| self.region_of(top));
        for area in areas {
            if let Some(part) = area.within(region) {
                self.invalidate(part);
            }
        }
        if let Some(mode) = mode {
            self.output.request_refresh(mode);
        }
    }

    /// The follow-up's areas, once the frame before it is drawn and nothing
    /// else waits to be: dropped when its screen is no longer on top.
    pub(crate) fn run_followup(&mut self) {
        if self.frame.followup.is_none()
            || self.frame.pending.is_some()
            || self.frame.unrendered.is_some()
            || self.output.redraw
        {
            return;
        }
        let Some(followup) = self.frame.followup.take() else {
            return;
        };
        if self
            .shared
            .stack
            .last()
            .is_some_and(|top| top.key == followup.screen)
        {
            self.redraw_areas(&followup.areas, followup.mode);
        }
    }

    /// Now, while a follow-up waits and the frame before it is drawn or
    /// about to be (`refreshing`): the shell comes back right after it. One
    /// whose frame before it is held back (a filter deferred it) waits for
    /// what wakes the shell anyway.
    pub(crate) fn followup_due(&self, refreshing: bool) -> Option<paperpaw_platform::Instant> {
        let drawn = self.frame.pending.is_none() && self.frame.unrendered.is_none();
        (self.frame.followup.is_some() && (refreshing || drawn)).then_some(self.now)
    }
}

/// Whether `command` is a `command.redraw_area`.
pub(super) fn is_area(command: &WireCommand<'_>) -> bool {
    matches!(command, WireCommand::RedrawArea { .. })
}

impl CommandStage<'_> {
    /// A mode as `command.redraw`'s, on the plugin's own top screen.
    pub(super) fn validate_redraw_area(&self, mode: u32) -> Result<usize, i32> {
        if mode > 3 {
            return Err(ERR_INVALID);
        }
        self.own_top().map(|_| 0)
    }

    /// Room for the callback's areas, taken once (`MAX_AREAS`), before the
    /// batch is accepted.
    pub(super) fn reserve_areas(&mut self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        if self.areas.list.capacity() > 0 || !batch.iter().any(|command| is_area(&command)) {
            return Ok(());
        }
        self.areas
            .list
            .try_reserve_exact(MAX_AREAS)
            .map_err(|_| ERR_NO_MEMORY)
    }

    /// Keeps the batch's redraws, whole and areas (accepted, room reserved).
    pub(super) fn push_redraws(&mut self, batch: &CommandBatch<'_>) {
        for command in batch.iter() {
            match command {
                WireCommand::Redraw { mode } => {
                    self.redraw = Some(self.redraw.map_or(mode, |old| old.max(mode)));
                }
                WireCommand::RedrawArea {
                    mode,
                    x,
                    y,
                    width,
                    height,
                } => self.areas.push(mode, Area::new(x, y, width, height)),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: i32, y: i32, width: u32, height: u32) -> Area {
        Area::new(x, y, width, height).unwrap_or(Area {
            x: 0,
            y: 0,
            right: 0,
            bottom: 0,
        })
    }

    #[test]
    fn an_area_is_clipped_to_the_region_and_one_outside_is_nothing() {
        let region = Rect::new(0, 40, 480, 720);
        assert_eq!(
            area(-10, 30, 100, 50).within(region),
            Some(Rect::new(0, 40, 90, 40))
        );
        assert_eq!(
            area(400, 700, 500, 500).within(region),
            Some(Rect::new(400, 700, 80, 60))
        );
        assert_eq!(area(0, 0, 480, 40).within(region), None);
        assert_eq!(
            area(i32::MAX, i32::MAX, u32::MAX, u32::MAX).within(region),
            None
        );
        assert_eq!(
            area(i32::MIN, i32::MIN, u32::MAX, u32::MAX).within(region),
            Some(region)
        );
        assert_eq!(Area::new(5, 5, 0, 9), None);
    }

    #[test]
    fn areas_past_the_kept_ones_join_the_last_and_modes_merge() {
        let mut areas = Areas::default();
        for i in 0..10_i32 {
            areas.push(u32::from(i == 4) * 2, Area::new(i * 10, 0, 5, 5));
        }
        assert_eq!(areas.mode, Some(2));
        assert_eq!(areas.list.len(), MAX_AREAS);
        assert_eq!(areas.list[MAX_AREAS - 1], area(70, 0, 25, 5));
        // An empty area asks for its mode and keeps nothing.
        areas.push(3, None);
        assert_eq!((areas.mode, areas.list.len()), (Some(3), MAX_AREAS));
    }
}
