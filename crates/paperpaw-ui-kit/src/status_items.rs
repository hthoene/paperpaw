//! Choosing what a status line shows: the model behind an editor screen for
//! any setting that lists status items or status lines, for a plugin to name
//! as its own screen (`EditorDecl::Screen`). It lists the items shown, in
//! their order, then the others; choosing a row shows or hides it (a newly
//! shown one goes last) and shown items are moved by dragging their grip or,
//! with buttons, by taking the focused one along: *Menu* picks it up, the
//! direction and page buttons move it a place, *Confirm*, *Back* or *Menu*
//! put it down. Items of plugins that are not there now stay in the setting
//! where they were.
//!
//! This is the behaviour of the interface-6 items screen as a model without
//! the host: it keeps the setting's ids, answers which rows to draw and what
//! the setting is after each change, and allocates fallibly (a refused
//! allocation is [`Error::NoMemory`] and changes nothing).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;

use crate::{mem, text};

/// An item or line that can be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its id, as the setting keeps it.
    pub id: String,
    /// The translation key of its name.
    pub title: String,
    /// The id a line had before, which a setting may still hold.
    pub also: Option<String>,
}

/// What an action meant to the chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not for the chooser: the list moves its focus.
    Ignored,
    /// Taken (a pick-up or put-down): redraw, the setting is as it was.
    Redraw,
    /// The setting changed; the focus goes to this row.
    Changed(usize),
}

/// The items to choose from and the ids the setting holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chooser {
    entries: Vec<Entry>,
    /// The ids of the setting, in order, as they were written.
    ids: Vec<String>,
    /// The focused shown item is being moved by the buttons.
    moving: bool,
}

impl Chooser {
    /// For `entries` and the setting's text (ids separated by commas).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(entries: Vec<Entry>, setting: &str) -> Result<Self, Error> {
        let mut ids = Vec::new();
        for id in setting
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            mem::push(&mut ids, text::copy(id)?)?;
        }
        Ok(Self {
            entries,
            ids,
            moving: false,
        })
    }

    /// The entries.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether the buttons are moving the focused item.
    #[must_use]
    pub const fn moving(&self) -> bool {
        self.moving
    }

    /// The entry `id` names, also by an id a line had before.
    fn entry_of(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.id == id || e.also.as_deref() == Some(id))
    }

    /// The entries shown, in their order, each once.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn chosen(&self) -> Result<Vec<usize>, Error> {
        let mut chosen = Vec::new();
        for id in &self.ids {
            match self.entry_of(id) {
                Some(entry) if !chosen.contains(&entry) => mem::push(&mut chosen, entry)?,
                _ => {}
            }
        }
        Ok(chosen)
    }

    /// The rows: indexes into the entries, the shown ones first.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn rows(&self) -> Result<Vec<usize>, Error> {
        let mut rows = self.chosen()?;
        let shown = rows.len();
        for entry in 0..self.entries.len() {
            if !rows.get(..shown).is_some_and(|s| s.contains(&entry)) {
                mem::push(&mut rows, entry)?;
            }
        }
        Ok(rows)
    }

    /// The setting as it is now: the ids joined by a comma and a space.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn value(&self) -> Result<String, Error> {
        join(self.ids.iter().map(String::as_str))
    }

    /// Keeps `chosen` (entries) as the items shown. Ids of the setting that
    /// no entry names keep their places among them; a known id's place is
    /// taken by the next of `chosen`; what is left of `chosen` goes last.
    fn keep(&mut self, chosen: &[usize]) -> Result<(), Error> {
        let mut left = chosen.iter();
        let mut ids: Vec<String> = Vec::new();
        for id in &self.ids {
            if self.entry_of(id).is_some() {
                if let Some(entry) = left.next() {
                    let id = self.entries.get(*entry).map_or("", |e| e.id.as_str());
                    mem::push(&mut ids, text::copy(id)?)?;
                }
            } else {
                mem::push(&mut ids, text::copy(id)?)?;
            }
        }
        for entry in left {
            let id = self.entries.get(*entry).map_or("", |e| e.id.as_str());
            mem::push(&mut ids, text::copy(id)?)?;
        }
        self.ids = ids;
        Ok(())
    }

    /// Shows or hides the entry of row `row` (a newly shown one goes last).
    /// `false` where there is no such row.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing changed.
    pub fn toggle(&mut self, row: usize) -> Result<bool, Error> {
        let Some(&entry) = self.rows()?.get(row) else {
            return Ok(false);
        };
        let mut chosen = self.chosen()?;
        match chosen.iter().position(|c| *c == entry) {
            Some(at) => {
                chosen.remove(at);
            }
            None => mem::push(&mut chosen, entry)?,
        }
        self.keep(&chosen)?;
        Ok(true)
    }

    /// Moves shown item `from` to place `to` (a drop of the grip). `false`
    /// where either is not a shown place or they are the same.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing changed.
    pub fn move_to(&mut self, from: usize, to: usize) -> Result<bool, Error> {
        let mut chosen = self.chosen()?;
        if from >= chosen.len() || to >= chosen.len() || from == to {
            return Ok(false);
        }
        let entry = chosen.remove(from);
        chosen.insert(to, entry);
        self.keep(&chosen)?;
        Ok(true)
    }

    /// Whether the buttons can take the item at `focus` along: the focus
    /// shows on a shown item and another is shown to move it past.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn can_move(&self, focus: usize, focus_shown: bool) -> Result<bool, Error> {
        let shown = self.chosen()?.len();
        Ok(focus_shown && focus < shown && shown > 1)
    }

    /// Drops what the buttons held, as a finger takes over from them.
    pub fn put_down(&mut self) {
        self.moving = false;
    }

    /// An action of the buttons with the focus on row `focus` (`focus_shown`:
    /// the focus shows). Not moving: *Menu* picks the focused shown item up
    /// and *Confirm* chooses the row (while the focus shows; else the caller
    /// shows it first). Moving: a direction or page button
    /// moves it a place within the shown ones; *Confirm*, *Back* and *Menu*
    /// put it down; anything else is not taken.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing changed.
    pub fn action(
        &mut self,
        action: &str,
        focus: usize,
        focus_shown: bool,
    ) -> Result<Outcome, Error> {
        if self.moving {
            let delta = match action {
                "core.up" | "core.left" | "core.previous" | "core.prev-page" => -1_isize,
                "core.down" | "core.right" | "core.next" | "core.next-page" => 1,
                "core.confirm" | "core.back" | "core.menu" => {
                    self.moving = false;
                    return Ok(Outcome::Redraw);
                }
                _ => return Ok(Outcome::Ignored),
            };
            let shown = self.chosen()?.len();
            let to = focus
                .saturating_add_signed(delta)
                .min(shown.saturating_sub(1));
            return Ok(if self.move_to(focus, to)? {
                Outcome::Changed(to)
            } else {
                Outcome::Redraw
            });
        }
        match action {
            "core.menu" if self.can_move(focus, focus_shown)? => {
                self.moving = true;
                Ok(Outcome::Redraw)
            }
            "core.confirm" if focus_shown => Ok(if self.toggle(focus)? {
                Outcome::Changed(focus)
            } else {
                Outcome::Redraw
            }),
            _ => Ok(Outcome::Ignored),
        }
    }
}

/// `ids` joined by a comma and a space, with fallible allocation.
fn join<'a>(ids: impl Iterator<Item = &'a str>) -> Result<String, Error> {
    let mut out = String::new();
    for (at, id) in ids.enumerate() {
        let room = id.len() + if at == 0 { 0 } else { 2 };
        out.try_reserve(room).map_err(|_| Error::NoMemory)?;
        if at > 0 {
            out.push_str(", ");
        }
        out.push_str(id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
