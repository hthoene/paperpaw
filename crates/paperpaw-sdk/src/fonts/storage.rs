//! Fonts read from the storage: packs read where they are, other font
//! files read into memory.

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::{Storage, StorageError};
use paperpaw_text::{Font, FontError, PackSource};

/// A pack on the storage, read where it is.
pub(super) struct Stored {
    pub(super) storage: Rc<dyn Storage>,
    pub(super) path: String,
}

impl PackSource for Stored {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            match self
                .storage
                .read_at(&self.path, at + done as u64, &mut buf[done..])
            {
                Ok(0) | Err(_) => return false,
                Ok(n) => done += n,
            }
        }
        true
    }
}

/// Reads a font file into memory: `Ok(None)` where it cannot be read or is
/// no font, [`FontError::OutOfMemory`] where the heap has no room for it.
pub(super) fn read_font(
    storage: &dyn Storage,
    path: &str,
    weight: Option<f32>,
) -> Result<Option<Font>, FontError> {
    let Ok(size) = storage.size(path) else {
        return Ok(None);
    };
    if size > super::MAX_FONT_FILE {
        return Ok(None);
    }
    let mut data: Vec<u8> = Vec::new();
    data.try_reserve_exact(size as usize)
        .map_err(|_| FontError::OutOfMemory)?;
    data.resize(size as usize, 0);
    let mut at = 0;
    while at < data.len() {
        match storage.read_at(path, at as u64, &mut data[at..]) {
            Ok(n) if n > 0 => at += n,
            _ => return Ok(None),
        }
    }
    Ok(match weight {
        Some(weight) => Font::from_data(data, &[("wght", weight)]).ok(),
        None => Font::from_data(data, &[]).ok(),
    })
}

/// Why a font file could not be opened ([`open_file`]).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// There is no such file.
    NotFound,
    /// The card said no (a read that failed, a file too large to take).
    Storage(StorageError),
    /// The file is no font (or no font this build opens), or the heap had no
    /// room for it ([`FontError::OutOfMemory`]).
    Font(FontError),
}

/// Opens the font file at `path` (a card path) for a plugin's own use, as a
/// book's fonts and the chosen family's are opened: a pack (`.ppfp`) is read
/// where it is, a glyph at a time, so only its index is kept; another file
/// (TrueType, OpenType) is read whole into memory, as large as
/// `MAX_FONT_FILE` at most, only in a build with outlines (the `outline`
/// feature; else [`FontError::OutlinesNotSupported`], before any of it is
/// read), and is where the heap has no room for it
/// [`FontError::OutOfMemory`].
///
/// # Errors
/// [`OpenError`].
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn open_file(
    storage: &Rc<dyn Storage>,
    path: &str,
    limits: paperpaw_text::PackLimits,
) -> Result<Font, OpenError> {
    let size = storage.size(path).map_err(|error| match error {
        StorageError::NotFound => OpenError::NotFound,
        other => OpenError::Storage(other),
    })?;
    if super::is_pack(path) {
        let source = Rc::new(Stored {
            storage: storage.clone(),
            path: super::loading::try_string(path).map_err(OpenError::Font)?,
        });
        return paperpaw_text::StoredPack::open_within(source, limits)
            .map(Font::Stored)
            .map_err(OpenError::Font);
    }
    // A build without outlines (the X4's) opens packs only: the file is
    // refused before any of it is read.
    if !cfg!(feature = "outline") {
        return Err(OpenError::Font(FontError::OutlinesNotSupported));
    }
    if size > super::MAX_FONT_FILE {
        return Err(OpenError::Storage(StorageError::TooLarge));
    }
    let mut data: Vec<u8> = Vec::new();
    data.try_reserve_exact(size as usize)
        .map_err(|_| OpenError::Font(FontError::OutOfMemory))?;
    data.resize(size as usize, 0);
    let mut at = 0;
    while at < data.len() {
        match storage.read_at(path, at as u64, &mut data[at..]) {
            Ok(n) if n > 0 => at += n,
            // A file that ends before its size said is read wrong.
            Ok(_) => return Err(OpenError::Storage(StorageError::Io)),
            Err(error) => return Err(OpenError::Storage(error)),
        }
    }
    Font::from_data(data, &[]).map_err(OpenError::Font)
}

#[cfg(test)]
#[path = "open_tests.rs"]
mod open_tests;
