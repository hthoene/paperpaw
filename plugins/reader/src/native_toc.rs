//! What the table-of-contents screen decides without a host: the entries made
//! from a book's headings where its own table is missing or has one entry,
//! the kept copy of them (valid while the book keeps its edition), the focus
//! on the chapter a place is in, and the parameters that
//! carry a place. The interface-6 `toc.rs` is the reference until it goes at
//! activation; every allocation here is fallible.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::{concat, copy};

/// Headings kept at most for a table of contents.
pub const MAX_HEADINGS: usize = 500;
/// The most the table of contents holds of its titles, in bytes counted by
/// [`row_cost`] (#248): a list of 500 long headings is over the X4's heap, so
/// the list stops here and says that the rest is left out. 500 titles of up
/// to 32 bytes fit ([`MAX_HEADINGS`] stops them first).
pub const TOC_BYTES: usize = 48 * 1024;
/// What a row of the list holds beside its title's bytes: its item and the
/// place it leads to.
pub const ROW_OVERHEAD: usize = 64;

/// What a heading titled `title` costs the list (see [`TOC_BYTES`]).
#[must_use]
pub const fn row_cost(title: &str) -> usize {
    title.len() + ROW_OVERHEAD
}

/// Blocks read in one step of finding the headings.
pub const STEP_BLOCKS: usize = 60;
/// Longest heading kept, in characters.
pub const MAX_TITLE: usize = 120;
/// Deepest level shown (a heading's level below the highest one found).
pub const MAX_LEVEL: u8 = 4;
/// Largest kept copy of the headings read back (500 headings of 120 characters
/// and their numbers fit; a larger file is not one this model wrote).
pub const MAX_KEPT: usize = 80 * 1024;

/// A place in a book.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Place {
    /// The section.
    pub section: u32,
    /// The offset in it.
    pub offset: u32,
}

impl Place {
    /// The place at `offset` in `section`.
    #[must_use]
    pub const fn new(section: u32, offset: u32) -> Self {
        Self { section, offset }
    }
}

/// One entry of a table of contents.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Entry {
    /// What the entry says.
    pub title: String,
    /// How deep it is (0 is the highest).
    pub level: u8,
    /// Where it leads.
    pub at: Place,
}

impl Entry {
    /// An entry.
    #[must_use]
    pub const fn new(title: String, level: u8, at: Place) -> Self {
        Self { title, level, at }
    }
}

/// The text of a heading as an entry's title: its words with single spaces,
/// at most [`MAX_TITLE`] characters; `None` where nothing is left.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn title_of(text: &str) -> Result<Option<String>, Error> {
    let mut title = String::new();
    let mut chars = 0;
    for word in text.split_whitespace() {
        if !title.is_empty() {
            if chars >= MAX_TITLE {
                break;
            }
            title.try_reserve(1).map_err(|_| Error::NoMemory)?;
            title.push(' ');
            chars += 1;
        }
        for c in word.chars() {
            if chars >= MAX_TITLE {
                break;
            }
            title
                .try_reserve(c.len_utf8())
                .map_err(|_| Error::NoMemory)?;
            title.push(c);
            chars += 1;
        }
    }
    Ok((!title.is_empty()).then_some(title))
}

/// Entries with their levels counted from the highest one found (0 for it),
/// none deeper than [`MAX_LEVEL`].
pub fn normalize(headings: &mut [Entry]) {
    let top = headings.iter().map(|h| h.level).min().unwrap_or(0);
    for h in headings {
        h.level = (h.level - top).min(MAX_LEVEL);
    }
}

/// The path of the kept headings of the book whose data folder is `book_dir`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn headings_path(book_dir: &str) -> Result<String, Error> {
    concat(&[book_dir, "/headings.txt"])
}

/// The kept copy of `headings` for a book's `edition` (its conversion; the
/// file size in interface 6): a line `size N`, then one line each of section,
/// offset, level and title, tab separated (the form interface 6 wrote).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn kept_text(edition: u64, headings: &[Entry]) -> Result<String, Error> {
    let mut out = String::new();
    push(&mut out, format_args!("size {edition}\n"))?;
    for h in headings {
        push(
            &mut out,
            format_args!(
                "{}\t{}\t{}\t{}\n",
                h.at.section, h.at.offset, h.level, h.title
            ),
        )?;
    }
    Ok(out)
}

fn push(out: &mut String, args: core::fmt::Arguments<'_>) -> Result<(), Error> {
    paperpaw_ui_kit::text::write_to(out, args)
}

/// Entries are added this many at a time: a list grows by a little, never
/// by double its length (#248).
pub const GROWTH: usize = 8;

/// Adds `entry` to `list`, growing it by [`GROWTH`] where it is full.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn push_entry(list: &mut Vec<Entry>, entry: Entry) -> Result<(), Error> {
    if list.len() == list.capacity() {
        list.try_reserve_exact(GROWTH)
            .map_err(|_| Error::NoMemory)?;
    }
    list.push(entry);
    Ok(())
}

/// Reads a kept copy a line at a time, so a whole copy is never held: the
/// first line names the edition it was made for, then one entry a line. It
/// stops one entry past [`TOC_BYTES`] (the list then says some are left out)
/// or at [`MAX_HEADINGS`].
#[derive(Debug)]
pub struct KeptReader {
    edition: u64,
    /// The first line was read and names `edition`.
    header: bool,
    /// The copy is not one for this edition.
    other: bool,
    found: Vec<Entry>,
    bytes: usize,
}

impl KeptReader {
    /// A reader of a copy for the book's `edition`.
    #[must_use]
    pub const fn new(edition: u64) -> Self {
        Self {
            edition,
            header: false,
            other: false,
            found: Vec::new(),
            bytes: 0,
        }
    }

    /// Takes the next `line`; `false` when no more are wanted. A line that is
    /// not an entry is left out.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn line(&mut self, line: &str) -> Result<bool, Error> {
        if self.other {
            return Ok(false);
        }
        if !self.header {
            let kept = line
                .strip_prefix("size ")
                .and_then(|n| n.parse::<u64>().ok());
            self.header = kept == Some(self.edition);
            self.other = !self.header;
            return Ok(self.header);
        }
        if self.found.len() >= MAX_HEADINGS || self.bytes > TOC_BYTES {
            return Ok(false);
        }
        let mut f = line.splitn(4, '\t');
        let (Some(section), Some(offset), Some(level), Some(title)) = (
            f.next().and_then(|v| v.parse().ok()),
            f.next().and_then(|v| v.parse().ok()),
            f.next().and_then(|v| v.parse().ok()),
            f.next(),
        ) else {
            return Ok(true);
        };
        self.bytes = self.bytes.saturating_add(row_cost(title));
        push_entry(
            &mut self.found,
            Entry::new(copy(title)?, level, Place::new(section, offset)),
        )?;
        Ok(true)
    }

    /// The headings read: `None` where the copy was for another edition or
    /// had no first line.
    #[must_use]
    pub fn finish(self) -> Option<Vec<Entry>> {
        (self.header && !self.other).then_some(self.found)
    }
}

/// The headings read back from a kept copy, when it was made for the book's
/// `edition`; `None` where it was made for another or is not one. A line
/// that is not an entry is left out.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn kept_headings(text: &str, edition: u64) -> Result<Option<Vec<Entry>>, Error> {
    if text.len() > MAX_KEPT {
        return Ok(None);
    }
    let mut reader = KeptReader::new(edition);
    for line in text.lines() {
        if !reader.line(line)? {
            break;
        }
    }
    Ok(reader.finish())
}

/// Whether the headings should be looked for: the book's own table has at
/// most one entry (`len`) and a kept copy, if any, is not better.
#[must_use]
pub const fn wants_scan(len: usize) -> bool {
    len <= 1
}

/// The table the screen shows: the kept headings when they have more entries
/// than the book's own table (`len`), else the book's own.
#[must_use]
pub fn prefer(headings: Option<Vec<Entry>>, len: usize) -> Option<Vec<Entry>> {
    headings.filter(|h| h.len() > len)
}

/// The index of the last entry at or before `at`, 0 where none is.
#[must_use]
pub fn current(entries: &[Entry], at: Place) -> usize {
    entries.iter().rposition(|h| h.at <= at).unwrap_or(0)
}

/// The index of the last entry of a book's own table at or before `at`: those
/// of an earlier section are before it, those of its section by their exact
/// target (`target`), later sections are after it. `sections[i]` is the section
/// of entry `i`.
pub fn current_of_table(
    sections: impl Iterator<Item = u32>,
    at: Place,
    mut target: impl FnMut(usize) -> Option<Place>,
) -> usize {
    let mut focus = 0;
    for (index, section) in sections.enumerate() {
        let before = match section.cmp(&at.section) {
            core::cmp::Ordering::Less => true,
            core::cmp::Ordering::Equal => target(index).is_some_and(|t| t <= at),
            core::cmp::Ordering::Greater => false,
        };
        if !before {
            break;
        }
        focus = index;
    }
    focus
}

/// Where reading on goes after a block that ended at `end` while the scan
/// stood at `at`: past the block, and at least one further.
#[must_use]
pub const fn next_place(at: Place, end: u32) -> Place {
    Place::new(
        at.section,
        if end > at.offset {
            end
        } else {
            at.offset.saturating_add(1)
        },
    )
}

/// The parameters of a screen that carries a place.
pub const SECTION: &str = "section";
/// See [`SECTION`].
pub const OFFSET: &str = "offset";

/// The place the parameters carry (both, decimal), else `None`.
#[must_use]
pub fn place_of(section: Option<&str>, offset: Option<&str>) -> Option<Place> {
    Some(Place::new(section?.parse().ok()?, offset?.parse().ok()?))
}

#[cfg(test)]
mod tests;
