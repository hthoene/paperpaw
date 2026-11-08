//! Bookmarks and the history on interface 7: lists of pages in the plugin's
//! data folder, one `title<TAB>address` line each, as the interface-6
//! browser wrote them (`src/store.rs`; version 1 files stay readable). The
//! paths are relative to the data folder (`DataStorage`, root 0). Every
//! allocation is fallible, and what is read is bounded:
//!
//! - a list file is read up to [`MAX_FILE`] bytes (64 KiB; interface 6
//!   allowed 256 KiB, more than the heap holds on the X4); a larger one is
//!   [`StorageError::TooLarge`] and is not written over;
//! - a list holds at most [`MAX_ENTRIES`] entries; more is `TooLarge` too;
//! - an address longer than [`MAX_URL`] bytes is dropped, as no request can
//!   carry it; a title is cut at [`MAX_TITLE`] bytes (on a character).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{Error, net::MAX_URL};
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_ui_kit::{mem, text};

use crate::native_web::page_name;

/// Pages kept in the history; older ones are forgotten, with their files.
pub const HISTORY: usize = 30;
/// The largest list file read.
pub const MAX_FILE: usize = 64 * 1024;
/// Entries a list holds at most.
pub const MAX_ENTRIES: usize = 400;
/// The longest title kept.
pub const MAX_TITLE: usize = 256;

/// The bookmarks file, in the data folder.
pub const BOOKMARKS: &str = "bookmarks.txt";
/// The history file, in the data folder.
pub const HISTORY_FILE: &str = "history.txt";
/// Where an answer is downloaded to before it goes where it belongs.
pub const DOWNLOAD: &str = "download.tmp";

/// Why a list could not be read or kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreError {
    /// A buffer could not be allocated.
    NoMemory,
    /// The storage refused (`TooLarge`, `InvalidData` for text that is not
    /// UTF-8, a failing card): the file is there and must not be written over.
    Storage(StorageError),
}

impl From<StorageError> for StoreError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<Error> for StoreError {
    fn from(_: Error) -> Self {
        Self::NoMemory
    }
}

/// A page in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    pub url: String,
}

impl Entry {
    /// An entry; the title cut at [`MAX_TITLE`].
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(title: &str, url: &str) -> Result<Self, Error> {
        let mut end = title.len().min(MAX_TITLE);
        while !title.is_char_boundary(end) {
            end -= 1;
        }
        Ok(Self {
            title: text::copy(&title[..end])?,
            url: text::copy(url)?,
        })
    }

    /// A copy.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            title: text::copy(&self.title)?,
            url: text::copy(&self.url)?,
        })
    }
}

/// Where the page at `url` is kept.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn page_path(url: &str) -> Result<String, Error> {
    text::concat(&["pages/", &page_name(url)?])
}

/// The whole file at `path` (at most `cap` bytes): `None` where there is none.
fn read_whole(
    storage: &dyn Storage,
    path: &str,
    cap: usize,
) -> Result<Option<Vec<u8>>, StoreError> {
    let size = match storage.size(path) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > cap {
        return Err(StorageError::TooLarge.into());
    }
    let mut data = mem::buffer(size)?;
    let mut filled = 0;
    while filled < size {
        let n = storage.read_at(path, filled as u64, &mut data[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    data.truncate(filled);
    Ok(Some(data))
}

/// A list, or `None` if it was never saved; an error where it is there but
/// cannot be read, so it is not written over.
///
/// # Errors
/// [`StoreError`].
pub fn load(storage: &dyn Storage, path: &str) -> Result<Option<Vec<Entry>>, StoreError> {
    let Some(data) = read_whole(storage, path, MAX_FILE)? else {
        return Ok(None);
    };
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data);
    let list = core::str::from_utf8(data).map_err(|_| StorageError::InvalidData)?;
    let mut entries = Vec::new();
    for (title, url) in list
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, url)| !url.is_empty() && url.len() <= MAX_URL)
    {
        if entries.len() == MAX_ENTRIES {
            return Err(StorageError::TooLarge.into());
        }
        mem::push(&mut entries, Entry::new(title, url)?)?;
    }
    Ok(Some(entries))
}

/// `entries` as the file holds them.
fn render(entries: &[Entry]) -> Result<String, Error> {
    let clean = |c: char| {
        if matches!(c, '\t' | '\n' | '\r') {
            ' '
        } else {
            c
        }
    };
    let mut len = 0usize;
    for entry in entries {
        len = len
            .saturating_add(entry.title.len())
            .saturating_add(entry.url.len())
            .saturating_add(2);
    }
    let mut out = String::new();
    out.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    for entry in entries {
        out.extend(entry.title.chars().map(clean));
        out.push('\t');
        out.extend(entry.url.chars().map(clean));
        out.push('\n');
    }
    Ok(out)
}

/// Writes `entries` to `path`.
///
/// # Errors
/// [`StoreError`]; the file is written whole or not at all by the storage.
pub fn save(storage: &dyn Storage, path: &str, entries: &[Entry]) -> Result<(), StoreError> {
    let text = render(entries)?;
    storage.write(path, text.as_bytes())?;
    Ok(())
}

/// The bookmarks; until the user changes them, a few places to start from, in
/// the language of the device (`language`: `de`, `de-AT` or `en_GB`). `None`
/// where they cannot be read: then none are shown, and none saved over them.
///
/// # Errors
/// [`StoreError::NoMemory`].
pub fn bookmarks(storage: &dyn Storage, language: &str) -> Result<Option<Vec<Entry>>, StoreError> {
    let saved = match load(storage, BOOKMARKS) {
        Ok(saved) => saved,
        Err(StoreError::NoMemory) => return Err(StoreError::NoMemory),
        Err(StoreError::Storage(_)) => return Ok(None),
    };
    if let Some(saved) = saved {
        return Ok(Some(saved));
    }
    let language = language.split(['-', '_']).next().unwrap_or("en");
    let wikipedia = text::concat(&["https://", language, ".m.wikipedia.org/"])?;
    let mut list = Vec::new();
    mem::push(&mut list, Entry::new("Wikipedia", &wikipedia)?)?;
    mem::push(
        &mut list,
        Entry::new("Project Gutenberg", "https://www.gutenberg.org/")?,
    )?;
    mem::push(
        &mut list,
        Entry::new("Standard Ebooks", "https://standardebooks.org/ebooks")?,
    )?;
    Ok(Some(list))
}

/// Puts `entry` first in the history; pages that drop out of it are deleted.
/// Where the history cannot be read, nothing changes: `Ok(None)`.
///
/// # Errors
/// [`StoreError::NoMemory`]; a refusal to write the list is
/// [`StoreError::Storage`] (no page file is removed then).
pub fn remember(storage: &dyn Storage, entry: Entry) -> Result<Option<Vec<Entry>>, StoreError> {
    let mut history = match load(storage, HISTORY_FILE) {
        Ok(history) => history.unwrap_or_default(),
        Err(StoreError::NoMemory) => return Err(StoreError::NoMemory),
        Err(StoreError::Storage(_)) => return Ok(None),
    };
    history.retain(|e| e.url != entry.url);
    history.try_reserve(1).map_err(|_| Error::NoMemory)?;
    history.insert(0, entry);
    let kept = HISTORY.min(history.len());
    // The list first: a cut in between leaves pages without a line (found
    // and dropped as stale files), never lines without their pages.
    save(storage, HISTORY_FILE, &history[..kept])?;
    for old in history.drain(kept..) {
        let _ = storage.remove(&page_path(&old.url)?);
    }
    Ok(Some(history))
}
