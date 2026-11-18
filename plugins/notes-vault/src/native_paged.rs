//! What every screen of the vault shares: a [`WindowList`] and whether its
//! focus shows. The focus shows at once only where the reader used buttons last
//! (`event.screen_shown.last_input`), and from the first button press on.
use paperpaw_sdk::Handled;
use paperpaw_sdk::keys::InputMode;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::{ItemStep, RowSource, WindowList};

/// A list of rows and its focus.
#[derive(Debug)]
pub struct Paged {
    list: WindowList,
    shown: bool,
}

impl Paged {
    /// A list whose rows have a line for a note.
    #[must_use]
    pub fn new() -> Self {
        Self {
            list: WindowList::new(true),
            shown: false,
        }
    }

    /// Shown again (or for the first time) after `last_input`.
    pub fn shown(&mut self, last_input: Option<InputMode>) {
        self.shown = last_input == Some(InputMode::Buttons);
    }

    /// Focuses the row `index`.
    pub fn set_focus(&mut self, index: usize) {
        self.list.set_focus(index);
    }

    /// Draws the page the focus is on under the catalog key `title` (a title
    /// that is no key is drawn as it is).
    ///
    /// # Errors
    /// Allocation, font, canvas, source and host errors; the geometry of the
    /// last completed draw stays where one fails.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        rows: &mut impl RowSource,
    ) -> Result<(), Error> {
        self.list.draw(host, frame, title, rows, self.shown)
    }

    /// Takes an input for a list of `len` rows: the focus moves, the header's
    /// action runs; a chosen row is returned. `Handled::No` for what is not the
    /// list's.
    ///
    /// # Errors
    /// Command queueing errors; the focus is unchanged then.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        len: usize,
        input: &Input<'_>,
    ) -> Result<(Handled, Option<usize>), Error> {
        if matches!(input, Input::Action { .. }) && !self.shown {
            // The first press only shows where the focus is.
            if self.list.decide(len, input) != ItemStep::Ignored {
                self.shown = true;
                let mut scratch = [0; 64];
                paperpaw_sdk::native::commands::CommandSender::new(host, &mut scratch)
                    .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)?;
                return Ok((Handled::Yes, None));
            }
        }
        Ok(match self.list.input(host, len, input)? {
            ItemStep::Chosen(index) => (Handled::Yes, Some(index)),
            ItemStep::Focus(_) | ItemStep::Header(_) => (Handled::Yes, None),
            _ => (Handled::No, None),
        })
    }
}

impl Default for Paged {
    fn default() -> Self {
        Self::new()
    }
}
