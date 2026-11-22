//! Focus, paging and the navigation actions of a list screen.
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind},
        commands::CommandSender,
        settings::{self, SettingValue},
    },
};

use crate::ListRenderer;

/// What an input did to a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListInput {
    /// Not for the list.
    Ignored,
    /// Focus or page moved; a redraw was requested.
    Moved,
    /// The row `index` was chosen (Confirm, or a tap on it).
    Chosen(usize),
    /// A header region ran an action (back, previous or next page): queued.
    Header,
}

/// The focused row of a list and how input moves it.
#[derive(Debug, Default)]
pub struct ListFocus {
    focus: usize,
    /// The arrow keys stay on the page (`keep_arrows_stay`).
    arrows_stay: Option<bool>,
}

impl ListFocus {
    /// The focused row.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Focuses row `index` (clamped on use).
    pub fn set(&mut self, index: usize) {
        self.focus = index;
    }

    /// Keeps the focus inside `len` rows.
    pub fn clamp(&mut self, len: usize) {
        self.focus = self.focus.min(len.saturating_sub(1));
    }

    /// What `input` means to a list of `len` rows drawn by `renderer`, without
    /// doing anything about it.
    ///
    /// Navigation actions move the focus (a page of rows for the page
    /// actions); Confirm chooses the focused row; a tap on a row chooses it; a
    /// tap in the header means back, previous or next page.
    #[must_use]
    pub fn decide(&self, renderer: &ListRenderer, len: usize, input: &Input<'_>) -> Step {
        match *input {
            Input::Action { action, .. } => {
                let rows = renderer.rows().max(1);
                let last = len.saturating_sub(1);
                let focus = self.focus.min(last);
                let stays = self.arrows_stay == Some(true);
                let target = match action {
                    "core.confirm" if len > 0 => return Step::Chosen(focus),
                    "core.down" | "core.next" | "core.right" => {
                        let below = focus.checked_add(1).filter(|to| *to < len);
                        step_in_pages(stays, focus, below, true, renderer.rows(), len)
                    }
                    "core.up" | "core.previous" | "core.left" => step_in_pages(
                        stays,
                        focus,
                        focus.checked_sub(1),
                        false,
                        renderer.rows(),
                        len,
                    ),
                    "core.next-page" => focus.saturating_add(rows),
                    "core.prev-page" => focus.saturating_sub(rows),
                    _ => return Step::Ignored,
                };
                Step::Focus(target.min(last))
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                if let Some(action) = renderer.header_action(position) {
                    Step::Header(action)
                } else {
                    renderer
                        .hit(position, len)
                        .map_or(Step::Ignored, Step::Chosen)
                }
            }
            _ => Step::Ignored,
        }
    }

    /// Interprets `input` as [`Self::decide`] says and does what it asks:
    /// moves the focus with a redraw, or runs the header's action.
    ///
    /// # Errors
    /// Propagates command queueing errors; the focus is unchanged then.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        renderer: &ListRenderer,
        len: usize,
        input: &Input<'_>,
    ) -> Result<ListInput, Error> {
        keep_arrows_stay(host, &mut self.arrows_stay, input);
        match self.decide(renderer, len, input) {
            Step::Ignored => Ok(ListInput::Ignored),
            Step::Chosen(index) => {
                self.focus = index;
                Ok(ListInput::Chosen(index))
            }
            Step::Header(action) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
                Ok(ListInput::Header)
            }
            Step::Focus(target) => {
                if target != self.focus {
                    // The row the focus leaves and the one it reaches, as on
                    // interface 6; the screen where the page turns.
                    renderer.redraw_step(host, self.focus, target)?;
                    self.focus = target;
                }
                Ok(ListInput::Moved)
            }
        }
    }
}

/// Keeps in `kept` the reader's choice that the arrow keys stay on the page of
/// a list (`core.arrows-stay-on-page`): read once, at the first key the list
/// takes, not at every one, and kept as long as the list (a screen builds its
/// list anew where it opens). A value that is no switch reads as on, the
/// setting's default; a refused read is tried again at the next key.
#[inline(never)]
pub(crate) fn keep_arrows_stay(host: &Host<'_>, kept: &mut Option<bool>, input: &Input<'_>) {
    if kept.is_some() || !matches!(input, Input::Action { .. }) {
        return;
    }
    let mut scratch = [0; 32];
    let key = paperpaw_sdk::builtin::settings::ARROWS_STAY_ON_PAGE.static_str();
    *kept = match settings::read_setting(host, key, &mut scratch) {
        Ok(SettingValue::Bool(on)) => Some(on),
        Ok(_) => Some(true),
        Err(_) => None,
    };
}

/// Where an arrow key that steps to `target` (onwards with `forward`; `None`
/// past the list's end) takes the focus where the arrow keys stay on the page
/// whose rows the focus may rest on run from `first` to `last`: there while it
/// is on the page, else to the page's other end, as in Beta 3. The page keys
/// still turn the page.
pub(crate) fn round_page(
    target: Option<usize>,
    forward: bool,
    (first, last): (usize, usize),
) -> usize {
    match target {
        Some(to) if (first..=last).contains(&to) => to,
        _ if forward => first,
        _ => last,
    }
}

/// Where an arrow key that steps from `focus` to `target` takes the focus in a
/// list of `len` rows that all take it, in pages of `rows` rows (0 before the
/// first frame): to the page's other end where the arrow keys stay on the page
/// (`stays`) and the step leaves it ([`round_page`]), else to `target`, or
/// nowhere past the list's ends.
pub(crate) fn step_in_pages(
    stays: bool,
    focus: usize,
    target: Option<usize>,
    forward: bool,
    rows: usize,
    len: usize,
) -> usize {
    if !stays || rows == 0 {
        return target.unwrap_or(focus);
    }
    let first = focus - focus % rows;
    let last = first.saturating_add(rows).min(len).saturating_sub(1);
    round_page(target, forward, (first, last))
}

/// What a list made of an input, before anything is done about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not for the list.
    Ignored,
    /// The focus should go to this row.
    Focus(usize),
    /// This row was chosen.
    Chosen(usize),
    /// The header's action should run.
    Header(&'static str),
}

/// `Handled::Yes` for what a list took.
#[must_use]
pub fn handled(input: ListInput) -> Handled {
    if input == ListInput::Ignored {
        Handled::No
    } else {
        Handled::Yes
    }
}

/// The focus of a part that is not a row of a list (a key, a card, a cell of
/// a grid, a letter): the theme's focus on exactly `rect`, a bar or, with a
/// ring focus, a frame (round a pill, where `rect` is one band high). A row
/// leaves room at its sides ([`crate::list::backdrop`]); this does not.
/// Returns the ink of what is drawn on it ([`crate::list::ink`]).
///
/// # Errors
/// [`paperpaw_sdk::native::Error::NoMemory`] when the paint list cannot grow.
pub fn frame(
    paint: &mut alloc::vec::Vec<crate::Paint>,
    style: &crate::Style,
    rect: paperpaw_sdk::Rect,
) -> Result<paperpaw_sdk::Color, paperpaw_sdk::native::Error> {
    let radius = if rect.width < rect.height || !style.pill || rect.height > style.row {
        style.corner
    } else {
        u32::from(rect.height) / 2
    };
    crate::paint::push(
        paint,
        crate::Paint::Focus(
            rect,
            radius,
            style
                .ring
                .then_some(u32::from(style.line) * 2)
                .map(|line| line.max(3)),
            style.bar(),
        ),
    )?;
    Ok(crate::list::ink(style, true))
}

/// The focus of a part that has content of its own (a card with a switch in
/// it, a widget's preview, a key over a heat map): always a frame, never a bar
/// the content would have to be redrawn on. In the theme's outline colour (the
/// text's ink where it set none), `(line * 2).max(3)` thick, with the style's
/// corners.
///
/// # Errors
/// [`paperpaw_sdk::native::Error::NoMemory`] when the paint list cannot grow.
pub fn ring(
    paint: &mut alloc::vec::Vec<crate::Paint>,
    style: &crate::Style,
    rect: paperpaw_sdk::Rect,
) -> Result<(), paperpaw_sdk::native::Error> {
    let thickness = (u32::from(style.line) * 2).max(3);
    let color = style.roles.outline.unwrap_or(style.ink);
    crate::paint::push(
        paint,
        crate::Paint::Focus(rect, style.corner, Some(thickness), color),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{Point, Rect};

    fn action(action: &str) -> Input<'_> {
        Input::Action { now_ms: 0, action }
    }
    fn tap(x: i32, y: i32) -> Input<'static> {
        Input::Gesture {
            now_ms: 0,
            position: Point::new(x, y),
            kind: InputGestureKind::Tap,
        }
    }

    #[test]
    fn navigation_moves_by_rows_and_pages_and_stops_at_the_ends() {
        let renderer = ListRenderer::geometry(Rect::new(0, 50, 300, 200), 40, 0, 5);
        let mut list = ListFocus::default();
        assert_eq!(
            list.decide(&renderer, 12, &action("core.down")),
            Step::Focus(1)
        );
        assert_eq!(
            list.decide(&renderer, 12, &action("core.up")),
            Step::Focus(0)
        );
        assert_eq!(
            list.decide(&renderer, 12, &action("core.next-page")),
            Step::Focus(5)
        );
        list.set(10);
        assert_eq!(
            list.decide(&renderer, 12, &action("core.next-page")),
            Step::Focus(11)
        );
        assert_eq!(
            list.decide(&renderer, 12, &action("core.prev-page")),
            Step::Focus(5)
        );
        assert_eq!(
            list.decide(&renderer, 12, &action("core.confirm")),
            Step::Chosen(10)
        );
        assert_eq!(
            list.decide(&renderer, 12, &action("core.menu")),
            Step::Ignored
        );
    }

    #[test]
    fn an_empty_list_chooses_nothing_and_a_stale_focus_is_clamped() {
        let renderer = ListRenderer::geometry(Rect::new(0, 50, 300, 200), 40, 0, 5);
        let mut list = ListFocus::default();
        assert_eq!(
            list.decide(&renderer, 0, &action("core.confirm")),
            Step::Ignored
        );
        list.set(40);
        assert_eq!(
            list.decide(&renderer, 3, &action("core.confirm")),
            Step::Chosen(2)
        );
        assert_eq!(
            list.decide(&renderer, 3, &action("core.down")),
            Step::Focus(2)
        );
    }

    #[test]
    fn where_the_arrows_stay_on_the_page_they_go_round_it() {
        // Two pages of five rows: 0-4 and 5-7.
        let renderer = ListRenderer::geometry(Rect::new(0, 50, 300, 200), 40, 0, 5);
        for stays in [false, true] {
            let at = |focus| ListFocus {
                focus,
                arrows_stay: Some(stays),
            };
            let down = |focus| at(focus).decide(&renderer, 8, &action("core.down"));
            let up = |focus| at(focus).decide(&renderer, 8, &action("core.up"));
            assert_eq!(down(4), Step::Focus(if stays { 0 } else { 5 }), "{stays}");
            assert_eq!(up(5), Step::Focus(if stays { 7 } else { 4 }), "{stays}");
            assert_eq!(down(7), Step::Focus(if stays { 5 } else { 7 }), "{stays}");
            assert_eq!(up(0), Step::Focus(if stays { 4 } else { 0 }), "{stays}");
            assert_eq!(down(2), Step::Focus(3), "{stays}");
            let page = at(4).decide(&renderer, 8, &action("core.next-page"));
            assert_eq!(page, Step::Focus(7), "{stays}");
        }
    }

    #[test]
    fn a_tap_chooses_the_row_under_it_and_nothing_beyond_the_rows() {
        let renderer = ListRenderer::geometry(Rect::new(0, 50, 300, 200), 40, 1, 5);
        let list = ListFocus::default();
        assert_eq!(list.decide(&renderer, 8, &tap(10, 50)), Step::Chosen(5));
        assert_eq!(list.decide(&renderer, 8, &tap(10, 130)), Step::Chosen(7));
        assert_eq!(list.decide(&renderer, 8, &tap(10, 170)), Step::Ignored);
        assert_eq!(list.decide(&renderer, 8, &tap(10, 10)), Step::Ignored);
    }
}
