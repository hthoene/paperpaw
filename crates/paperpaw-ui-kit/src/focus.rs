//! Focus, paging and the navigation actions of a list screen.
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind},
        commands::CommandSender,
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
                let target = match action {
                    "core.confirm" if len > 0 => return Step::Chosen(focus),
                    "core.down" | "core.next" | "core.right" => focus.saturating_add(1),
                    "core.up" | "core.previous" | "core.left" => focus.saturating_sub(1),
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
    fn a_tap_chooses_the_row_under_it_and_nothing_beyond_the_rows() {
        let renderer = ListRenderer::geometry(Rect::new(0, 50, 300, 200), 40, 1, 5);
        let list = ListFocus::default();
        assert_eq!(list.decide(&renderer, 8, &tap(10, 50)), Step::Chosen(5));
        assert_eq!(list.decide(&renderer, 8, &tap(10, 130)), Step::Chosen(7));
        assert_eq!(list.decide(&renderer, 8, &tap(10, 170)), Step::Ignored);
        assert_eq!(list.decide(&renderer, 8, &tap(10, 10)), Step::Ignored);
    }
}
