//! Where each book was left, shared like its annotations: any reader
//! keeps it, and plugins read it (a sync, a list of books) without
//! opening the book or knowing the reader. A file per book in
//! `.paperpaw/positions`, moved and removed with the book.
//!
//! ```ini
//! [position]
//! section = 7
//! offset = 1234
//! percent = 41.5
//! conversion = 00000000000000ff
//! xpointer = "/body/DocFragment[8]/body/p[3]/text()[1].12"
//! updated = 1760000000
//! book = 482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b
//! ```
//!
//! `book` is which book it is of by its content, its [`Fingerprint`]
//! (the file is named by the book's path): a reader keeps it with
//! [`save_with_book`], and [`book`] reads it.

use alloc::collections::TryReserveError;
use alloc::format;
use alloc::string::String;

use crate::data::Error;
use crate::document::Locator;
use crate::library::Fingerprint;
use crate::{Storage, StorageError};

/// The folder of the positions (registered as book data by the kernel).
pub const DIR: &str = crate::data_path!("positions");

/// The largest position file read.
const LIMIT: usize = 4096;

/// Where a book was left.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Position {
    /// The start of the page shown.
    pub at: Locator,
    /// How far into the book, in percent.
    pub percent: f32,
    /// Which conversion of the book `at` is in, as the reader names it
    /// (its options, see [`reading::open_options`](crate::reading::open_options)):
    /// offsets of one conversion need not fit another.
    pub conversion: Option<u64>,
    /// The place as an XPointer, which fits every conversion (and
    /// KOReader), where the reader kept one: when the book was closed.
    pub xpointer: Option<String>,
    /// When it was left, in seconds since 1970 (0 where the clock was not
    /// known).
    pub updated: i64,
}

impl Position {
    pub fn new(at: Locator, percent: f32) -> Self {
        Self {
            at,
            percent,
            conversion: None,
            xpointer: None,
            updated: 0,
        }
    }

    pub fn conversion(mut self, conversion: u64) -> Self {
        self.conversion = Some(conversion);
        self
    }

    pub fn xpointer(mut self, xpointer: impl Into<Option<String>>) -> Self {
        self.xpointer = xpointer.into();
        self
    }

    pub fn updated(mut self, updated: i64) -> Self {
        self.updated = updated;
        self
    }

    /// Read from a position file's text (also a reader's `progress.ini`
    /// of before, whose section is `[progress]`).
    pub fn parse(text: &str) -> Option<Position> {
        match try_parse(text, None) {
            Ok(position) => position,
            // Where there is no memory the program stops, as it always did.
            Err(_) => alloc::alloc::handle_alloc_error(core::alloc::Layout::new::<Position>()),
        }
    }

    /// As a position file's text.
    pub fn text(&self) -> String {
        let mut text = format!(
            "[position]\nsection = {}\noffset = {}\npercent = ",
            self.at.section, self.at.offset
        );
        // As `{:.1}` writes it, without `core`'s formatter of floats (#258).
        let _ = crate::float_text::write_tenths(&mut text, self.percent);
        text.push('\n');
        if let Some(conversion) = self.conversion {
            text.push_str(&format!("conversion = {conversion:016x}\n"));
        }
        if let Some(xpointer) = &self.xpointer {
            text.push_str(&format!("xpointer = {}\n", crate::config::quote(xpointer)));
        }
        if self.updated != 0 {
            text.push_str(&format!("updated = {}\n", self.updated));
        }
        text
    }
}

/// The position file of the book at `book`.
pub fn file(book: &str) -> String {
    format!("{DIR}/{}.txt", crate::library::key(book))
}

/// Where the book at `book` was left, if it was kept and can be read.
pub fn load(storage: &dyn Storage, book: &str) -> Option<Position> {
    load_for_update(storage, book).ok().flatten()
}

/// Where the book at `book` was left, `Ok(None)` if none was kept: an
/// error where the file is there but cannot be read (or is no position),
/// so it is not written over before the reader moves on.
pub fn load_for_update(
    storage: &dyn Storage,
    book: &str,
) -> Result<Option<Position>, StorageError> {
    let data = match crate::read_file(storage, &file(book), LIMIT) {
        Ok(data) => data,
        Err(StorageError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let text = core::str::from_utf8(crate::files::without_bom(&data))
        .map_err(|_| StorageError::InvalidData)?;
    Position::parse(text)
        .map(Some)
        .ok_or(StorageError::InvalidData)
}

/// The position file of the book at `book`, its name had fallibly.
fn try_file(book: &str) -> Result<String, TryReserveError> {
    let mut digits = [0_u8; crate::library::KEY_LEN];
    let key = crate::library::key_into(book, &mut digits);
    let mut name = String::new();
    name.try_reserve_exact(DIR.len() + 1 + key.len() + ".txt".len())?;
    for part in [DIR, "/", key, ".txt"] {
        name.push_str(part);
    }
    Ok(name)
}

/// The longest XPointer (as the file writes it, escapes and all) that
/// [`try_load`] holds: the bound of `doc_xpointer`, 1,024 bytes. A longer one
/// (only a hand-edited file has one) is treated as absent: never held and
/// never cut, and the rest of the position is read.
pub const XPOINTER_MAX: usize = 1024;

/// [`Position::parse`] with the memory error: the XPointer is the only
/// thing held, in one fallible reservation of its length (resolved at once,
/// so that an escaped one is not copied twice). With `limit`, a longer one
/// (as written) is left out before anything is reserved.
fn try_parse(text: &str, limit: Option<usize>) -> Result<Option<Position>, Error> {
    let (mut section, mut offset, mut percent) = (None, None, 0.0);
    let mut position = Position::new(Locator::START, 0.0);
    for entry in crate::config::parse(text).filter_map(Result::ok) {
        if entry.section != "position" && entry.section != "progress" {
            continue;
        }
        match entry.key {
            "section" => section = entry.value.as_u32(),
            "offset" => offset = entry.value.as_u32(),
            "percent" => percent = crate::float_text::parse_f32(entry.value.raw()).unwrap_or(0.0),
            "conversion" => position.conversion = u64::from_str_radix(entry.value.raw(), 16).ok(),
            "xpointer" => {
                if limit.is_some_and(|limit| entry.value.raw().len() > limit) {
                    // Treated as absent (a later, shorter one may still come).
                    position.xpointer = None;
                    continue;
                }
                let owned = match entry.value.try_as_str().map_err(|_| Error::OutOfMemory)? {
                    alloc::borrow::Cow::Owned(owned) => owned,
                    alloc::borrow::Cow::Borrowed(text) => {
                        let mut owned = String::new();
                        owned
                            .try_reserve_exact(text.len())
                            .map_err(|_| Error::OutOfMemory)?;
                        owned.push_str(text);
                        owned
                    }
                };
                position.xpointer = Some(owned);
            }
            "updated" => position.updated = entry.value.as_i64().unwrap_or(0),
            _ => {}
        }
    }
    let (Some(section), Some(offset)) = (section, offset) else {
        return Ok(None);
    };
    position.at = Locator::new(section, offset);
    position.percent = percent;
    Ok(Some(position))
}

/// [`load_for_update`] with the memory error: where the book was left,
/// `Ok(None)` if none was kept. Held at most: the file's text (4 KiB, as a
/// rule well under 1 KiB) and the XPointer ([`XPOINTER_MAX`], 1 KiB).
///
/// # Errors
/// [`Error::OutOfMemory`] where either cannot be had; [`Error::Unreadable`]
/// for a file that is no position; the storage's error otherwise. An
/// XPointer past [`XPOINTER_MAX`] is not an error: the position has none.
pub fn try_load(storage: &dyn Storage, book: &str) -> Result<Option<Position>, Error> {
    let file = try_file(book).map_err(|_| Error::OutOfMemory)?;
    let data = match crate::read_file(storage, &file, LIMIT) {
        Ok(data) => data,
        Err(StorageError::NotFound) => return Ok(None),
        Err(StorageError::OutOfMemory) => return Err(Error::OutOfMemory),
        Err(e) => return Err(e.into()),
    };
    drop(file);
    let text =
        core::str::from_utf8(crate::files::without_bom(&data)).map_err(|_| Error::Unreadable)?;
    try_parse(text, Some(XPOINTER_MAX))?
        .map(Some)
        .ok_or(Error::Unreadable)
}

/// Keeps where the book at `book` was left.
pub fn save(storage: &dyn Storage, book: &str, position: &Position) -> Result<(), StorageError> {
    save_with_book(storage, book, position, None)
}

/// Keeps where the book at `book` was left, and which book it is of by
/// its content, `fingerprint`, where it is known.
pub fn save_with_book(
    storage: &dyn Storage,
    book: &str,
    position: &Position,
    fingerprint: Option<&Fingerprint>,
) -> Result<(), StorageError> {
    let _ = storage.create_dir(DIR);
    let mut text = position.text();
    if let Some(fingerprint) = fingerprint {
        text.push_str(&format!("book = {}\n", fingerprint.text()));
    }
    storage.write(&file(book), text.as_bytes())
}

/// Which book the position kept of the book at `book` is of, by its
/// content, where it says.
pub fn book(storage: &dyn Storage, book: &str) -> Option<Fingerprint> {
    let data = crate::read_file(storage, &file(book), LIMIT).ok()?;
    let text = core::str::from_utf8(crate::files::without_bom(&data)).ok()?;
    crate::config::parse(text)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "position" && entry.key == "book")
        .and_then(|entry| Fingerprint::parse(&entry.value.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn positions_are_kept_and_read_back() {
        let storage = MemoryStorage::new();
        assert_eq!(load(&storage, "books/a.epub"), None);
        let position = Position::new(Locator::new(7, 1234), 41.5)
            .conversion(255)
            .xpointer(String::from("/body/DocFragment[8]/body/p[3]/text()[1].12"))
            .updated(1_760_000_000);
        save(&storage, "books/a.epub", &position).unwrap();
        assert_eq!(load(&storage, "books/a.epub"), Some(position.clone()));
        assert!(file("books/a.epub").starts_with(".paperpaw/positions/"));
        assert_eq!(book(&storage, "books/a.epub"), None);
        // With which book it is of; read as before by those who do not
        // ask.
        let fingerprint = Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b");
        save_with_book(&storage, "books/a.epub", &position, fingerprint.as_ref()).unwrap();
        assert_eq!(load(&storage, "books/a.epub"), Some(position));
        assert_eq!(book(&storage, "books/a.epub"), fingerprint);
    }

    /// A reader's `progress.ini` of version 1 reads as a position.
    #[test]
    fn a_progress_file_of_before_is_read() {
        let old = "[progress]\nsection = 7\noffset = 1234\npercent = 41.5\n";
        let position = Position::parse(old).unwrap();
        assert_eq!(position, Position::new(Locator::new(7, 1234), 41.5));
        assert_eq!(Position::parse("[position]\nsection = 1\n"), None);
    }

    /// A position file that is there but cannot be read is no missing
    /// one: it is not to be written over.
    #[test]
    fn a_position_that_cannot_be_read_is_told_from_none() {
        let book = "books/a.epub";
        let storage = MemoryStorage::new();
        assert_eq!(load_for_update(&storage, book), Ok(None));
        storage
            .write(&file(book), b"[position]\nsection = 1\noff")
            .unwrap();
        assert_eq!(
            load_for_update(&storage, book),
            Err(StorageError::InvalidData)
        );
        assert_eq!(load(&storage, book), None);
        let failing = crate::memory_storage::Unreadable(
            MemoryStorage::new().with(&file(book), "[position]\nsection = 1\noffset = 2\n"),
        );
        assert!(load_for_update(&failing, book).is_err());
    }

    /// The fallible read says what the plain one does, and every
    /// refused allocation of it is the memory error.
    #[test]
    fn a_position_is_read_whole_or_refused_for_every_allocation() {
        use crate::native::commands::menu_alloc_tests::refuse_nth;
        let book = "books/a.epub";
        let storage = MemoryStorage::new();
        assert_eq!(try_load(&storage, book), Ok(None));
        let position = Position::new(Locator::new(7, 1234), 41.5)
            .conversion(255)
            .xpointer(String::from(
                "/body/DocFragment[8]/body/p[3]/text()[1].12 \"quoted\"",
            ))
            .updated(1_760_000_000);
        save(&storage, book, &position).unwrap();
        assert_eq!(try_load(&storage, book), Ok(Some(position.clone())));
        assert_eq!(try_load(&storage, book).unwrap(), load(&storage, book));
        let mut allocations = 0;
        for number in 1.. {
            let (result, refused) = refuse_nth(number, || try_load(&storage, book));
            if !refused {
                assert_eq!(result, Ok(Some(position.clone())));
                allocations = number - 1;
                break;
            }
            assert_eq!(result, Err(Error::OutOfMemory), "refusal {number}");
        }
        // The name, the text and the XPointer.
        assert!(allocations >= 3, "{allocations}");
        // An XPointer past the bound is left out before it is held (the plain
        // read still has it), and one at the bound is read.
        let longest =
            Position::new(Locator::new(1, 2), 1.0).xpointer(Some("x".repeat(XPOINTER_MAX)));
        save(&storage, book, &longest).unwrap();
        assert_eq!(try_load(&storage, book), Ok(Some(longest)));
        let too_long =
            Position::new(Locator::new(1, 2), 1.0).xpointer(Some("x".repeat(XPOINTER_MAX + 1)));
        save(&storage, book, &too_long).unwrap();
        assert_eq!(
            try_load(&storage, book),
            Ok(Some(Position::new(Locator::new(1, 2), 1.0)))
        );
        assert_eq!(load(&storage, book), Some(too_long));
        // A file that is no position is unreadable, never none.
        storage
            .write(&file(book), b"[position]\nsection = 1\n")
            .unwrap();
        assert_eq!(try_load(&storage, book), Err(Error::Unreadable));
        storage.write(&file(book), &[0xff, 0xfe]).unwrap();
        assert_eq!(try_load(&storage, book), Err(Error::Unreadable));
    }
}
