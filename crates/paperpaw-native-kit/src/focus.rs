//! Focus, paging and the navigation actions of a list screen.
use paperpaw_sdk::{
    Handled, Point,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
    },
};

use crate::ListRenderer;

/// What an input meant to a list.
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

    /// Interprets `input` for a list of `len` rows drawn by `renderer`.
    ///
    /// Navigation actions move the focus (a page at a time for the page
    /// actions); Confirm chooses the focused row; a tap on a row focuses and
    /// chooses it; a tap in the header runs back / previous / next page.
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        renderer: &ListRenderer,
        len: usize,
        input: &Input<'_>,
    ) -> Result<ListInput, Error> {
        match *input {
            Input::Action { action, .. } => self.action(host, renderer, len, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(host, renderer, len, position),
            _ => Ok(ListInput::Ignored),
        }
    }

    fn action(
        &mut self,
        host: &Host<'_>,
        renderer: &ListRenderer,
        len: usize,
        action: &str,
    ) -> Result<ListInput, Error> {
        let rows = renderer.rows().max(1);
        let target = match action {
            "core.confirm" if len > 0 => {
                self.clamp(len);
                return Ok(ListInput::Chosen(self.focus));
            }
            "core.down" | "core.next" | "core.right" => self.focus.saturating_add(1),
            "core.up" | "core.previous" | "core.left" => self.focus.saturating_sub(1),
            "core.next-page" => self.focus.saturating_add(rows),
            "core.prev-page" => self.focus.saturating_sub(rows),
            _ => return Ok(ListInput::Ignored),
        };
        self.moved(host, target, len)
    }

    fn tap(
        &mut self,
        host: &Host<'_>,
        renderer: &ListRenderer,
        len: usize,
        position: Point,
    ) -> Result<ListInput, Error> {
        if let Some(action) = renderer.header_action(position) {
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch).run_action(action)?;
            return Ok(ListInput::Header);
        }
        match renderer.hit(position, len) {
            Some(index) => {
                self.focus = index;
                Ok(ListInput::Chosen(index))
            }
            None => Ok(ListInput::Ignored),
        }
    }

    fn moved(&mut self, host: &Host<'_>, target: usize, len: usize) -> Result<ListInput, Error> {
        let target = target.min(len.saturating_sub(1));
        if target != self.focus {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
            self.focus = target;
        }
        Ok(ListInput::Moved)
    }
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
