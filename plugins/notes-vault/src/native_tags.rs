//! The tags of the vault, each with how many notes have it, from the file of
//! tags the index makes (`native_model::index`).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::keys::InputMode;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::text::{concat, copy};
use paperpaw_ui_kit::{Messages, RowItem, RowSource};

use crate::native_model::index;
use crate::native_paged::Paged;

/// The tags screen.
#[derive(Debug)]
pub struct Tags {
    paged: Paged,
    tags: Vec<(String, u32)>,
}

impl Tags {
    /// The tags there are now.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>) -> Result<Self, Error> {
        let (tags, _) = index::tag_counts(&DataStorage::new(host))?;
        Ok(Self {
            paged: Paged::new(),
            tags,
        })
    }

    /// Shown again after `last_input`.
    pub fn shown(&mut self, last_input: Option<InputMode>) {
        self.paged.shown(last_input);
    }

    /// Rows on the screen: the tags, or the one line that says there are none.
    fn len(&self) -> usize {
        self.tags.len().max(1)
    }

    /// Draws the tags.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut paged = core::mem::take(&mut self.paged);
        let drawn = paged.draw(host, frame, "notes-vault.tags", &mut Rows { tags: &*self });
        self.paged = paged;
        drawn
    }

    /// Takes an input; the tag chosen, if one was, is returned for a list of
    /// its notes.
    ///
    /// # Errors
    /// Allocation and host errors; nothing is staged then.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<(Handled, Option<String>), Error> {
        let (handled, chosen) = self.paged.input(host, self.len(), input)?;
        let tag = match chosen.and_then(|index| self.tags.get(index)) {
            Some((tag, _)) => Some(copy(tag)?),
            None => None,
        };
        Ok((handled, tag))
    }
}

/// The rows of the screen for a draw.
struct Rows<'a> {
    tags: &'a Tags,
}

impl RowSource for Rows<'_> {
    fn len(&self) -> usize {
        self.tags.len()
    }

    fn fill(
        &mut self,
        host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        if self.tags.tags.is_empty() {
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push(RowItem {
                title: messages.copy(host, "notes-vault.empty.tags")?,
                ..RowItem::default()
            });
            return Ok(());
        }
        for (tag, count) in self.tags.tags.iter().skip(first).take(max) {
            let note =
                messages.format(host, "notes-vault.notes", &[Arg::Count(i64::from(*count))])?;
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push(RowItem {
                title: concat(&["#", tag])?,
                value: copy("›")?,
                note,
                ..RowItem::default()
            });
        }
        Ok(())
    }
}
