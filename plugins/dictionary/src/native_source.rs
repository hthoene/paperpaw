//! The StarDict dictionaries of a folder as the provider's [`Source`]: found
//! through a [`Listing`], read through a `Storage`. A dictionary whose indexes
//! are not made yet is not ready; its [`Builder`]s are what the background
//! work runs.
use crate::{
    native_disk::{cache_of, failed_before, model_error, stamp},
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
    /// Where its block indexes are (a path of [`crate::native_disk::Routed`]).
    cache: String,
    ready: bool,
}

/// One block index still to be made, with what a failure of it is kept by.
#[derive(Debug)]
pub struct Pending {
    pub builder: Builder,
    /// The folder of the dictionary's indexes.
    pub cache: String,
    /// What the files still to be indexed are, by their sizes
    /// ([`crate::native_disk::stamp`]): a failed dictionary is tried again
    /// only when they change.
    pub stamp: String,
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
                push(
                    &mut opened,
                    Opened {
                        dictionary,
                        cache,
                        ready,
                    },
                )?;
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

    /// The block indexes still to be made, over all the dictionaries, except
    /// those of a dictionary that could not be indexed before from the same
    /// files (its `failed` marker names them).
    ///
    /// # Errors
    /// The folder's, or `NoMemory`.
    pub fn pending(&mut self) -> Result<Vec<Pending>, Error> {
        let storage = self.storage;
        let mut all = Vec::new();
        for opened in self.open()?.iter().filter(|o| !o.ready) {
            let missing = opened.dictionary.builders(storage);
            let stamp = stamp(&missing)?;
            if failed_before(storage, &opened.cache, &stamp) {
                continue;
            }
            for builder in missing {
                push(
                    &mut all,
                    Pending {
                        builder,
                        cache: copy(&opened.cache)?,
                        stamp: copy(&stamp)?,
                    },
                )?;
            }
        }
        Ok(all)
    }

    /// The folders of the indexes of all the dictionaries found.
    ///
    /// # Errors
    /// The folder's, or `NoMemory`.
    pub fn caches(&mut self) -> Result<Vec<String>, Error> {
        let mut all = Vec::new();
        for opened in self.open()? {
            push(&mut all, copy(&opened.cache)?)?;
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
