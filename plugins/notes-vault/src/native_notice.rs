//! The screen a screen becomes where the card cannot be read when it opens: one
//! line that says why, with the header's back action, so the screen is never
//! left dead.
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::keys::InputMode;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::{Messages, RowItem, RowSource};

use crate::native_paged::Paged;

/// Why the screen has nothing to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The card access is not allowed (`uses.storage` denied).
    Denied,
    /// The card, or what is on it, could not be read.
    Unreadable,
}

impl Why {
    /// What a refusal of a screen's first read is told as; `None` for
    /// [`Error::NoMemory`], which the host handles.
    #[must_use]
    pub const fn of(error: &Error) -> Option<Self> {
        match error {
            Error::NoMemory => None,
            Error::Denied => Some(Self::Denied),
            _ => Some(Self::Unreadable),
        }
    }

    /// The catalog key of the line that says it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Denied => "notes-vault.status.denied",
            Self::Unreadable => "notes-vault.status.unreadable",
        }
    }
}

/// The notice screen.
#[derive(Debug)]
pub struct Notice {
    paged: Paged,
    why: Why,
}

impl Notice {
    /// A screen that says `why`.
    #[must_use]
    pub fn new(why: Why) -> Self {
        Self {
            paged: Paged::new(),
            why,
        }
    }

    /// Shown again after `last_input`.
    pub fn shown(&mut self, last_input: Option<InputMode>) {
        self.paged.shown(last_input);
    }

    /// Draws the line under the title of the vault's page.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut paged = core::mem::take(&mut self.paged);
        let drawn = paged.draw(host, frame, "notes-vault.title", &mut Line(self.why));
        self.paged = paged;
        drawn
    }

    /// Takes an input: the header's back action; the line is no choice.
    ///
    /// # Errors
    /// Command queueing errors.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        Ok(self.paged.input(host, 1, input)?.0)
    }
}

/// The one line.
struct Line(Why);

impl RowSource for Line {
    fn len(&self) -> usize {
        1
    }

    fn fill(
        &mut self,
        host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error> {
        if first > 0 || max == 0 {
            return Ok(());
        }
        out.try_reserve(1).map_err(|_| Error::NoMemory)?;
        out.push(RowItem {
            title: Messages::new()?.copy(host, self.0.key())?,
            ..RowItem::default()
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denial_is_told_apart_and_a_refused_allocation_is_not_a_notice() {
        assert_eq!(Why::of(&Error::Denied), Some(Why::Denied));
        assert_eq!(Why::of(&Error::NotFound), Some(Why::Unreadable));
        assert_eq!(Why::of(&Error::Io), Some(Why::Unreadable));
        assert_eq!(Why::of(&Error::NoMemory), None);
    }
}
