//! Which font files are opened for the roles plugins and readers bind.

use alloc::vec::Vec;

use paperpaw_text::FontError;

use super::{Fonts, Id};

#[cfg(any(test, feature = "kernel"))]
impl Fonts {
    /// The font of `family`'s regular face, open or not: the one a widget
    /// set in that family is drawn in.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn family_regular(&self, family: &str) -> Option<&Id> {
        self.best_face(family, false, false, false)
            .map(|index| &self.entries[index].id)
    }

    /// Keeps the files of `fonts` open beside those the roles and the
    /// chosen families want, in place of the ones kept before, and loads as
    /// [`Fonts::try_load`] does (its list of files that would not open is
    /// returned): fonts the reader chose for widgets, which no role names.
    /// A font kept is bound to itself, so `Fonts` grows by no field.
    ///
    /// # Errors
    /// [`FontError::OutOfMemory`]: what was kept and open before stays so.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn try_keep_open(
        &mut self,
        fonts: &[Id],
        storage: &alloc::rc::Rc<dyn paperpaw_platform::Storage>,
    ) -> Result<Vec<alloc::string::String>, FontError> {
        let no_room = |_| FontError::OutOfMemory;
        // The ones of `fonts` not kept yet, each once.
        let mut new: Vec<&Id> = Vec::new();
        new.try_reserve_exact(fonts.len()).map_err(no_room)?;
        for font in fonts {
            if self.user.get(font.as_str()) != Some(font) && !new.contains(&font) {
                new.push(font);
            }
        }
        let mut added: Vec<(Id, Id)> = Vec::new();
        added.try_reserve_exact(new.len()).map_err(no_room)?;
        for font in &new {
            let pair = font.try_clone().zip(font.try_clone());
            added.push(pair.ok_or(FontError::OutOfMemory)?);
        }
        self.user.try_reserve_exact(added.len()).map_err(no_room)?;
        // Room was made: none of these allocates.
        for (role, font) in added {
            self.user.insert(role, font);
        }
        // The new ones are opened while the old ones still are, so that a
        // shortage leaves the widgets what they had.
        let failed = match self.try_load(storage) {
            Ok(failed) => failed,
            Err(error) => {
                for font in new {
                    self.user.remove(font.as_str());
                }
                return Err(error);
            }
        };
        self.user
            .retain(|role, font| role != font || fonts.contains(role));
        // Opens nothing: closes what is kept no more (at the next load,
        // where the heap has no room for this one's list).
        let _closed = self.try_load(storage);
        Ok(failed)
    }
}

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
