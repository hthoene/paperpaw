//! The StarDict dictionaries of a folder as the provider's [`Source`]: found
//! through a [`Listing`], read through a `Storage`. A dictionary whose indexes
//! are not made yet is not ready; its [`Builder`]s are what the background
//! work runs.
use crate::{
    native_disk::{cache_of, model_error},
    native_model::{self, Found, Listing, Source},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Storage, native::Error};
use paperpaw_stardict::{Builder, Dictionary};
use paperpaw_ui_kit::{mem::push, text::copy};

/// One dictionary found.
#[derive(Debug)]
struct Opened {
    dictionary: Dictionary,
    ready: bool,
}

/// The dictionaries below a folder.
pub struct Dictionaries<'a, L: Listing> {
    listing: &'a mut L,
    storage: &'a dyn Storage,
    folder: String,
    opened: Option<Vec<Opened>>,
}

impl<L: Listing> core::fmt::Debug for Dictionaries<'_, L> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dictionaries")
            .field("folder", &self.folder)
            .finish_non_exhaustive()
    }
}

impl<'a, L: Listing> Dictionaries<'a, L> {
    /// The dictionaries below `folder` (without its outer `/`).
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the folder's name cannot be kept.
    pub fn new(listing: &'a mut L, storage: &'a dyn Storage, folder: &str) -> Result<Self, Error> {
        Ok(Self {
            listing,
            storage,
            folder: copy(folder.trim_matches('/'))?,
            opened: None,
        })
    }

    fn open(&mut self) -> Result<&[Opened], Error> {
        if self.opened.is_none() {
            let mut opened = Vec::new();
            for ifo in native_model::find_dictionaries(self.listing, &self.folder)? {
                let cache = cache_of(&ifo)?;
                // A dictionary that cannot be read is left out, a refused
                // allocation is not.
                let dictionary = match Dictionary::open(self.storage, &ifo, &cache) {
                    Ok(dictionary) => dictionary,
                    Err(error) if model_error(&error) == Error::NoMemory => {
                        return Err(Error::NoMemory);
                    }
                    Err(_) => continue,
                };
                let ready = dictionary.builders(self.storage).is_empty();
                push(&mut opened, Opened { dictionary, ready })?;
            }
            self.opened = Some(opened);
        }
        Ok(self.opened.as_deref().unwrap_or_default())
    }

    /// How many dictionaries were found, ready or not.
    ///
    /// # Errors
    /// The folder's, or `NoMemory`.
    pub fn found(&mut self) -> Result<usize, Error> {
        Ok(self.open()?.len())
    }

    /// The block indexes still to be made, over all the dictionaries.
    ///
    /// # Errors
    /// The folder's, or `NoMemory`.
    pub fn builders(&mut self) -> Result<Vec<Builder>, Error> {
        let storage = self.storage;
        let mut all = Vec::new();
        for opened in self.open()?.iter().filter(|o| !o.ready) {
            for builder in opened.dictionary.builders(storage) {
                push(&mut all, builder)?;
            }
        }
        Ok(all)
    }
}

impl<L: Listing> Source for Dictionaries<'_, L> {
    fn ready(&mut self) -> Result<usize, Error> {
        Ok(self.open()?.iter().filter(|o| o.ready).count())
    }

    fn definitions(&mut self, which: usize, word: &str) -> Result<Vec<Found>, Error> {
        let storage = self.storage;
        let opened = self.open()?.iter().filter(|o| o.ready).nth(which);
        let Some(opened) = opened else {
            return Ok(Vec::new());
        };
        let found = opened
            .dictionary
            .lookup(storage, word)
            .map_err(|error| model_error(&error))?;
        named(&opened.dictionary, found)
    }

    fn definitions_within(
        &mut self,
        which: usize,
        word: &str,
        budget: &mut usize,
    ) -> Result<Vec<Found>, Error> {
        let storage = self.storage;
        let opened = self.open()?.iter().filter(|o| o.ready).nth(which);
        let Some(opened) = opened else {
            return Ok(Vec::new());
        };
        let found = opened
            .dictionary
            .lookup_within(storage, word, budget)
            .map_err(|error| model_error(&error))?;
        named(&opened.dictionary, found)
    }
}

/// `found`, each definition with the name of its dictionary.
fn named(
    dictionary: &Dictionary,
    found: Vec<paperpaw_stardict::Definition>,
) -> Result<Vec<Found>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(found.len())
        .map_err(|_| Error::NoMemory)?;
    for definition in found {
        out.push(Found {
            title: copy(&dictionary.info.name)?,
            text: definition.text,
        });
    }
    Ok(out)
}
