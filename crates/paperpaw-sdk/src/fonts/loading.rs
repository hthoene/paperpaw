//! Opening the font files of the chosen families, and choosing them, where
//! the heap may be short: the kernel refreshes fonts while running (a
//! plugin installed, a setting changed) and keeps what it had where it
//! cannot.

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;
use paperpaw_text::{Font, FontError, StoredPack};

use super::{Fonts, Stored, is_pack, read_font};

/// `text` copied, or [`FontError::OutOfMemory`] where the heap has no room.
pub(super) fn try_string(text: &str) -> Result<String, FontError> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| FontError::OutOfMemory)?;
    copy.push_str(text);
    Ok(copy)
}

impl Fonts {
    /// Whether a font of `family` is registered.
    pub fn has_family(&self, family: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.family.as_deref() == Some(family))
    }

    /// Whether `family` can set text now: it has a regular face that is
    /// built in or open, or in a file of the plugin that named it (taken
    /// as there, as ever), or in a file of another plugin's folder that
    /// is on `storage`. A family of styles alone, or one whose regular
    /// face another plugin has yet to bring, is not offered or taken for
    /// reading: its letters would be another font's.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn family_there(&self, family: &str, storage: &dyn Storage) -> bool {
        // Only a file of another plugin's folder is looked for: one look
        // at the card for each such family, none for the others.
        let own = |entry: &super::Entry, file: &str| {
            let folder = file
                .strip_prefix(crate::builtin::PLUGIN_DIR)
                .and_then(|rest| rest.strip_prefix('/'))
                .and_then(|rest| rest.split('/').next());
            folder.is_none_or(|folder| folder == entry.id.namespace())
        };
        self.entries.iter().any(|entry| {
            let there = |file: &str| own(entry, file) || storage.size(file).is_ok();
            entry.family.as_deref() == Some(family)
                && !entry.bold
                && !entry.italic
                && (entry.font.is_some() || entry.file.as_deref().is_some_and(there))
        })
    }

    /// [`Fonts::families`], or [`FontError::OutOfMemory`] where the heap has
    /// no room for the list.
    ///
    /// # Errors
    /// [`FontError::OutOfMemory`].
    pub fn try_families(&self) -> Result<Vec<&str>, FontError> {
        let mut families: Vec<&str> = Vec::new();
        for family in self.entries.iter().filter_map(|e| e.family.as_deref()) {
            if !families.contains(&family) {
                families
                    .try_reserve(1)
                    .map_err(|_| FontError::OutOfMemory)?;
                families.push(family);
            }
        }
        Ok(families)
    }

    /// [`Fonts::set_family`] where the heap may be short: the family chosen
    /// before is returned (to give back with [`Fonts::restore_family`] if
    /// what follows fails), and kept where there is no room for the name.
    ///
    /// # Errors
    /// [`FontError::OutOfMemory`].
    pub fn try_set_family(&mut self, family: Option<&str>) -> Result<Option<String>, FontError> {
        let family = family.map(try_string).transpose()?;
        Ok(core::mem::replace(&mut self.family, family))
    }

    /// Loads the files of the chosen family's faces (one per style) and
    /// frees all others; returns the files that could not be loaded.
    /// Packs that extend fonts are opened too (they stay where they are).
    /// Where the heap is short nothing changes and the list is empty: see
    /// [`Fonts::try_load`] for the kernel's use.
    pub fn load(&mut self, storage: &Rc<dyn Storage>) -> Vec<String> {
        self.try_load(storage).unwrap_or_default()
    }

    /// Gives back the family [`Fonts::try_set_family`] returned.
    pub fn restore_family(&mut self, previous: Option<String>) {
        self.family = previous;
    }

    /// [`Fonts::set_status`] where the heap may be short: the choice made
    /// before is returned (to give back with [`Fonts::restore_status`]).
    ///
    /// # Errors
    /// [`FontError::OutOfMemory`].
    pub fn try_set_status(&mut self, choice: &str) -> Result<Option<String>, FontError> {
        let choice = try_string(choice)?;
        Ok(self.status.replace(choice))
    }

    /// Gives back the choice [`Fonts::try_set_status`] returned.
    pub fn restore_status(&mut self, previous: Option<String>) {
        self.status = previous;
    }

    /// The entries whose files are wanted open: the chosen family's faces
    /// (one per style), the status lines' family and the roles' faces.
    fn wanted_entries(&self) -> Result<Vec<usize>, FontError> {
        let no_room = |_| FontError::OutOfMemory;
        let mut wanted: Vec<usize> = Vec::new();
        if let Some(family) = self.family.as_deref() {
            wanted.try_reserve(4).map_err(no_room)?;
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                wanted.extend(self.best_face(family, bold, italic, false));
            }
        }
        // The status lines' family of their own: its regular face.
        if let Some(index) = self
            .status_family()
            .and_then(|family| self.best_face(family, false, false, false))
        {
            wanted.try_reserve(1).map_err(no_room)?;
            wanted.push(index);
        }
        self.role_faces(&mut wanted)?;
        Ok(wanted)
    }

    /// [`Fonts::load`] where the heap may be short. A shortage fails the
    /// call after the glyph cache was let go and the files tried once more;
    /// the fonts are then as they were (what is wanted is opened before
    /// anything is freed, and what this call opened is closed again), so
    /// that it can be made again later, as the kernel does at its next
    /// refresh. A font that cannot be read is no shortage: it is in the list.
    ///
    /// # Errors
    /// [`FontError::OutOfMemory`].
    pub fn try_load(&mut self, storage: &Rc<dyn Storage>) -> Result<Vec<String>, FontError> {
        match self.load_once(storage) {
            Err(FontError::OutOfMemory) if self.cache_bytes() > 0 => {
                // The glyphs are drawn again; the pack that will not open is
                // dearer.
                self.cache.borrow_mut().clear();
                self.load_once(storage)
            }
            done => done,
        }
    }

    fn load_once(&mut self, storage: &Rc<dyn Storage>) -> Result<Vec<String>, FontError> {
        let wanted = self.wanted_entries()?;
        let mut opened: Vec<usize> = Vec::new();
        let missing = self
            .entries
            .iter()
            .enumerate()
            .filter(|(index, entry)| {
                entry.file.is_some()
                    && entry.font.is_none()
                    && (entry.extends.is_some() || wanted.contains(index))
            })
            .count();
        opened
            .try_reserve_exact(missing)
            .map_err(|_| FontError::OutOfMemory)?;
        match self.open_missing(storage, &wanted, &mut opened) {
            Ok(failed) => {
                // Freed last, so that a failure above leaves the fonts as
                // they were.
                for (index, entry) in self.entries.iter_mut().enumerate() {
                    if entry.file.is_some() && entry.extends.is_none() && !wanted.contains(&index) {
                        entry.font = None;
                    }
                }
                Ok(failed)
            }
            Err(error) => {
                for index in opened {
                    self.entries[index].font = None;
                }
                Err(error)
            }
        }
    }

    /// Opens the files of `wanted` (and of the extensions) that are not
    /// open, noting each in `opened`, which has room for them all.
    fn open_missing(
        &mut self,
        storage: &Rc<dyn Storage>,
        wanted: &[usize],
        opened: &mut Vec<usize>,
    ) -> Result<Vec<String>, FontError> {
        let mut failed: Vec<String> = Vec::new();
        for index in 0..self.entries.len() {
            let entry = &self.entries[index];
            let Some(path) = &entry.file else {
                continue;
            };
            let extends = entry.extends.is_some();
            if entry.font.is_some() || !(extends || wanted.contains(&index)) {
                continue;
            }
            let font = if extends || (entry.face && is_pack(path)) {
                // A pack not there is not opened (nor is the `Rc` below
                // made: stable Rust has no fallible `Rc`, so the one
                // allocation of a few dozen bytes per pack opened is the
                // only one here that cannot be refused).
                match storage.size(path) {
                    Ok(_) => {
                        let source = Rc::new(Stored {
                            storage: storage.clone(),
                            path: try_string(path)?,
                        });
                        match StoredPack::open_within(source, self.pack_limits) {
                            Ok(pack) => Some(Font::Stored(pack)),
                            Err(FontError::OutOfMemory) => return Err(FontError::OutOfMemory),
                            Err(_) => None,
                        }
                    }
                    Err(_) => None,
                }
            } else {
                // A variable font at its style's weight.
                let weight = entry.face.then(|| f32::from(entry.weight));
                read_font(storage.as_ref(), path, weight)?
            };
            match font {
                Some(font) => {
                    self.entries[index].font = Some(font);
                    // Room was made for every one that can open.
                    opened.push(index);
                }
                None => {
                    let path = try_string(self.entries[index].file.as_deref().unwrap_or(""))?;
                    failed.try_reserve(1).map_err(|_| FontError::OutOfMemory)?;
                    failed.push(path);
                }
            }
        }
        Ok(failed)
    }
}
