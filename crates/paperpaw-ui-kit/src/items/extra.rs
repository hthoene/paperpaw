//! What only some rows have: a slider's track, a bold part of the title and
//! the − and + of a number that has the focus, behind the one thin pointer of [`RowItem::extra`].
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
    /// A number changed with Left and Right (screens without touch): while
    /// the row has the focus, its value is shown between round − and + as on
    /// a [`RowItem::stepper`], as Beta 3 showed it. Its title is laid out
    /// with their room either way, so the row keeps its height.
    pub focus_steps: bool,
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

    /// Whether the row shows − and + round its value while it has the focus
    /// (see [`RowExtra::focus_steps`]).
    #[must_use]
    pub fn focus_steps(&self) -> bool {
        self.extra
            .as_deref()
            .is_some_and(|extra| extra[0].focus_steps)
    }

    /// Whether the row shows its − and + now: always as a stepper, or as a
    /// number with the focus.
    pub(super) fn steps_shown(&self, focused: bool) -> bool {
        self.stepper || (focused && self.focus_steps())
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

impl Item {
    /// This row as a number changed with Left and Right: its − and + show
    /// while it has the focus (see [`RowExtra::focus_steps`]); as it was where
    /// the item is a heading.
    ///
    /// # Errors
    /// [`Error::NoMemory`] only.
    pub fn with_focus_steps(mut self) -> Result<Self, Error> {
        if let Self::Row(row) = &mut self {
            row.extra_mut()?.focus_steps = true;
        }
        Ok(self)
    }
}
