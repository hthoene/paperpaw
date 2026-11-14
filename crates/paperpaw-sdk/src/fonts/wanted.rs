//! Which font files are opened for the roles plugins and readers bind.

use alloc::vec::Vec;

use paperpaw_text::FontError;

use super::{Fonts, Id};

impl Fonts {
    /// The entries of the faces that take a role no chosen family takes:
    /// the user's choice, else the best proposal (as `proposed` picks it
    /// once loaded). Losing proposals stay unloaded, and so does a bold or
    /// italic proposed for book text of another family than the regular
    /// face's (`bound` leaves it unused): a card's styles of Literata
    /// where the UI font sets book text.
    #[cfg(any(test, feature = "kernel"))]
    pub(super) fn role_faces(&self, wanted: &mut Vec<usize>) -> Result<(), FontError> {
        let no_room = |_| FontError::OutOfMemory;
        let position = |font: &Id| self.entries.iter().position(|e| e.id == *font);
        wanted.try_reserve(self.user.len()).map_err(no_room)?;
        wanted.extend(self.user.values().filter_map(position));
        let best = |role: &Id| {
            self.candidates
                .iter()
                .filter(|(r, _, _)| r == role)
                .filter_map(|(_, font, priority)| Some((position(font)?, *priority)))
                .max_by_key(|(_, priority)| *priority)
                .map(|(entry, _)| entry)
        };
        // The entry regular book text will be in, as `role` finds it once
        // the files are open.
        let reading = super::roles::READING;
        let book = (self.user.get(reading.as_str()).and_then(position))
            .or_else(|| self.best_face(self.family.as_deref()?, false, false, false))
            .or_else(|| best(&reading));
        let family = |entry: usize| self.entries[entry].family.as_deref();
        let mut seen: Vec<&Id> = Vec::new();
        for (role, _, _) in &self.candidates {
            if seen.contains(&role)
                || self.user.contains_key(role.as_str())
                || self.family_face(role.as_str()).is_some()
            {
                continue;
            }
            seen.try_reserve(1).map_err(no_room)?;
            seen.push(role);
            let Some(entry) = best(role) else { continue };
            let other = book.is_some_and(|book| family(book) != family(entry));
            if super::is_book_style(role.as_str()) && other {
                continue;
            }
            wanted.try_reserve(1).map_err(no_room)?;
            wanted.push(entry);
        }
        Ok(())
    }
}
