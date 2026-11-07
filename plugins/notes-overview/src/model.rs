//! What *All notes* decides without a host: which books are listed and in
//! which order, how a book's counts read, and the text of the
//! `My Clippings.txt` export (see `clippings` in the interface-6 plugin for
//! the format's description).
//!
//! The file is written into a [`Sink`], a piece at a time, so a book with many
//! notes never needs more than the piece in hand. Every allocation is
//! fallible: a refusal comes back as [`Error::NoMemory`], never a panic.
//!
//! Notes arrive as plain [`Note`] values (the host's rows copied out by the
//! caller), so this module knows nothing of the library service.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::time;

/// The folder beside the library that exports go to.
pub const FOLDER: &str = "Exports";
/// The file, as the Kindle names it.
pub const NAME: &str = "My Clippings.txt";
/// Books listed at most (the rest are not shown, but are exported).
pub const MOST_BOOKS: usize = 200;
/// Written out when the text held is this long, so a book with many notes
/// does not fill the memory.
pub const FLUSH: usize = 6 * 1024;

const BOM: &str = "\u{feff}";
const SEPARATOR: &str = "==========";
const LINE: &str = "\r\n";

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A text destination that can refuse.
pub trait Sink {
    /// Appends `text`.
    ///
    /// # Errors
    /// [`Error::NoMemory`] (or the destination's own refusal) where it does
    /// not fit; nothing of `text` is kept then.
    fn put(&mut self, text: &str) -> Result<(), Error>;
}

impl Sink for String {
    fn put(&mut self, text: &str) -> Result<(), Error> {
        self.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
        self.push_str(text);
        Ok(())
    }
}

/// An owned copy of `text`, made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.put(text)?;
    Ok(out)
}

/// The kind of a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    Highlight,
    Bookmark,
    /// A kind a newer reader keeps; it is not listed and not exported.
    Other,
}

/// One note of a book, as the caller read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note<'a> {
    pub kind: Kind,
    /// How far in the book, 0 to 100.
    pub percent: u8,
    /// Seconds since 1970 (UTC); 0 where not known.
    pub created: i64,
    pub updated: i64,
    /// The highlighted text, or the bookmark's label.
    pub text: &'a str,
    /// The reader's own note on a highlight; empty if none.
    pub note: &'a str,
}

/// A book with notes, and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    pub path: String,
    pub title: String,
    pub highlights: u16,
    pub bookmarks: u16,
}

/// How many highlights and bookmarks `notes` hold.
#[must_use]
pub fn count<'a>(notes: impl IntoIterator<Item = &'a Note<'a>>) -> (u16, u16) {
    let (mut highlights, mut bookmarks) = (0u16, 0u16);
    for note in notes {
        match note.kind {
            Kind::Highlight => highlights = highlights.saturating_add(1),
            Kind::Bookmark => bookmarks = bookmarks.saturating_add(1),
            Kind::Other => {}
        }
    }
    (highlights, bookmarks)
}

/// The books with notes, by title.
#[derive(Debug, Default)]
pub struct Listing {
    books: Vec<Counted>,
    more: bool,
}

impl Listing {
    /// An empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            books: Vec::new(),
            more: false,
        }
    }

    /// Adds a book with `highlights` and `bookmarks`; a book without notes is
    /// skipped, and books past [`MOST_BOOKS`] only set [`Listing::more`].
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the list is unchanged then.
    pub fn add(
        &mut self,
        path: &str,
        title: &str,
        highlights: u16,
        bookmarks: u16,
    ) -> Result<(), Error> {
        if u32::from(highlights) + u32::from(bookmarks) == 0 {
            return Ok(());
        }
        if self.books.len() >= MOST_BOOKS {
            self.more = true;
            return Ok(());
        }
        let book = Counted {
            path: copy(path)?,
            title: copy(title)?,
            highlights,
            bookmarks,
        };
        self.books.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.books.push(book);
        Ok(())
    }

    /// Puts the books in order: by title without regard to case, equal titles
    /// by path.
    pub fn sort(&mut self) {
        self.books.sort_unstable_by(|a, b| {
            cmp_lowercase(&a.title, &b.title).then_with(|| a.path.cmp(&b.path))
        });
    }

    /// The books listed.
    #[must_use]
    pub fn books(&self) -> &[Counted] {
        &self.books
    }

    /// Whether books were left out of the list (the export has them all).
    #[must_use]
    pub const fn more(&self) -> bool {
        self.more
    }

    /// How many rows the screen has: the books and the *Export* row, or none
    /// without books.
    #[must_use]
    pub fn rows(&self) -> usize {
        match self.books.len() {
            0 => 0,
            n => n + 1,
        }
    }
}

/// Compares two texts by their lower-case characters, without allocating.
#[must_use]
pub fn cmp_lowercase(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// The pieces of `3 highlights · 1 bookmark`: which of the two counts show and
/// with what number, in the order shown. The screen turns each into its
/// plural text (`notes-overview.highlights`, `notes-overview.bookmarks`) and
/// joins them with [`COUNT_SEPARATOR`].
#[must_use]
pub fn count_parts(book: &Counted) -> [Option<(&'static str, u16)>; 2] {
    [
        (book.highlights > 0).then_some(("notes-overview.highlights", book.highlights)),
        (book.bookmarks > 0).then_some(("notes-overview.bookmarks", book.bookmarks)),
    ]
}

/// Between the highlight and the bookmark count.
pub const COUNT_SEPARATOR: &str = " · ";

/// Where the export goes: `Exports` beside the library folder `library`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn export_folder(library: &str) -> Result<String, Error> {
    let trimmed = library.trim_end_matches('/');
    let parent = match trimmed.rfind('/') {
        Some(0) => "",
        Some(at) => trimmed.get(..at).unwrap_or(""),
        None => "",
    };
    let mut out = String::new();
    if !parent.is_empty() {
        out.put(parent)?;
        out.put("/")?;
    }
    out.put(FOLDER)?;
    Ok(out)
}

/// The start of the file: the byte order mark.
///
/// # Errors
/// The sink's refusal.
pub fn start(out: &mut dyn Sink) -> Result<(), Error> {
    out.put(BOM)
}

/// `seconds` (local, since 1970) as the Kindle says it:
/// `Sunday, 1 November 2026 10:05:00`.
///
/// # Errors
/// The sink's refusal.
pub fn put_date(out: &mut dyn Sink, local: i64) -> Result<(), Error> {
    let days = local.div_euclid(86_400);
    let (year, month, day) = time::civil_from_days(days);
    let in_day = local.rem_euclid(86_400);
    out.put(
        WEEKDAYS
            .get(usize::from(time::weekday_of(days)))
            .copied()
            .unwrap_or(""),
    )?;
    out.put(", ")?;
    put_number(out, i64::from(day), 1)?;
    out.put(" ")?;
    out.put(
        MONTHS
            .get(usize::from(month.saturating_sub(1)))
            .copied()
            .unwrap_or(""),
    )?;
    out.put(" ")?;
    put_number(out, i64::from(year), 1)?;
    out.put(" ")?;
    put_number(out, in_day / 3600, 2)?;
    out.put(":")?;
    put_number(out, in_day % 3600 / 60, 2)?;
    out.put(":")?;
    put_number(out, in_day % 60, 2)
}

/// `value` in decimal, padded with zeros to `width` digits.
fn put_number(out: &mut dyn Sink, value: i64, width: usize) -> Result<(), Error> {
    let mut digits = [b'0'; 20];
    let mut at = digits.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        if let Some(slot) = digits.get_mut(at) {
            *slot = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    let end = digits.len();
    while end - at < width && at > 0 {
        at -= 1;
    }
    if value < 0 {
        out.put("-")?;
    }
    out.put(
        digits
            .get(at..)
            .and_then(|d| core::str::from_utf8(d).ok())
            .unwrap_or(""),
    )
}

/// `text` in one line, as the file has lines of its own.
fn put_one_line(out: &mut dyn Sink, text: &str) -> Result<(), Error> {
    let mut first = true;
    for word in text.split_whitespace() {
        if !first {
            out.put(" ")?;
        }
        first = false;
        out.put(word)?;
    }
    Ok(())
}

/// The first line of an entry: the book as the Kindle names it.
fn put_book_line(out: &mut dyn Sink, title: &str, author: &str) -> Result<(), Error> {
    put_one_line(out, title)?;
    if author.split_whitespace().next().is_some() {
        out.put(" (")?;
        put_one_line(out, author)?;
        out.put(")")?;
    }
    Ok(())
}

/// One entry: what it is, where, when, and its text (none for a bookmark).
fn put_entry(
    out: &mut dyn Sink,
    book: (&str, &str),
    what: &str,
    note: &Note<'_>,
    text: &str,
    local_when: i64,
) -> Result<(), Error> {
    put_book_line(out, book.0, book.1)?;
    out.put(LINE)?;
    out.put("- Your ")?;
    out.put(what)?;
    out.put(" at location ")?;
    let percent = i64::from(note.percent);
    put_number(out, percent, 1)?;
    // The Kindle gives a highlight a range of locations, the rest one.
    if what == "Highlight" {
        out.put("-")?;
        put_number(out, percent, 1)?;
    }
    out.put(" | Added on ")?;
    put_date(out, local_when)?;
    out.put(LINE)?;
    out.put(LINE)?;
    put_one_line(out, text)?;
    out.put(LINE)?;
    out.put(SEPARATOR)?;
    out.put(LINE)
}

/// Writes the entries of the notes of one book, by position, and returns how
/// many entries there are. A highlight's own note is an entry of its own after
/// it. `local` turns the seconds since 1970 (UTC) of a note into the local
/// ones; a time not known (0) is the start of 1970.
///
/// # Errors
/// The sink's refusal; part of the book may have been written then.
pub fn put_book(
    out: &mut dyn Sink,
    local: &dyn Fn(i64) -> i64,
    title: &str,
    author: &str,
    notes: &[Note<'_>],
) -> Result<u32, Error> {
    let book = (title, author);
    let mut count = 0u32;
    for note in notes {
        let at = if note.created > 0 {
            note.created
        } else {
            note.updated
        };
        let when = if at > 0 { local(at) } else { 0 };
        match note.kind {
            Kind::Highlight => {
                put_entry(out, book, "Highlight", note, note.text, when)?;
                count += 1;
                if !note.note.is_empty() {
                    put_entry(out, book, "Note", note, note.note, when)?;
                    count += 1;
                }
            }
            Kind::Bookmark => {
                put_entry(out, book, "Bookmark", note, "", when)?;
                count += 1;
            }
            Kind::Other => {}
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
