//! What only some rows have: a slider's track and a bold part of the title,
//! behind the one thin pointer of [`RowItem::extra`].
use super::*;

/// What only some rows have: [`RowItem::extra`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowExtra {
    /// A number shown with a slider under its row (touch screens): its value
    /// and range. The row is a stepper too; the track takes a second row.
    pub track: Option<Track>,
    /// The bytes of the title drawn bold where they are shown (the word found
    /// in a search hit's snippet); `None` for a title all in one weight.
    pub bold: Option<(u16, u16)>,
}

impl RowItem {
    /// The slider under the row, if it has one.
    #[must_use]
    pub fn track(&self) -> Option<Track> {
        self.extra.as_deref().and_then(|extra| extra[0].track)
    }

    /// The bytes of the title drawn bold, if any.
    #[must_use]
    pub fn bold(&self) -> Option<(u16, u16)> {
        self.extra.as_deref().and_then(|extra| extra[0].bold)
    }

    /// The extras of this row, made on first use.
    ///
    /// # Errors
    /// [`Error::NoMemory`] only.
    pub(super) fn extra_mut(&mut self) -> Result<&mut RowExtra, Error> {
        if self.extra.is_none() {
            let mut one = Vec::new();
            one.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
            one.push(RowExtra::default());
            self.extra = one.into_boxed_slice().try_into().ok();
        }
        self.extra
            .as_deref_mut()
            .map(|extra| &mut extra[0])
            .ok_or(Error::NoMemory)
    }
}
