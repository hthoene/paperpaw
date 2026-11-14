//! Where a reader is in a book: its progress, from the lengths of the sections
//! (`screens-input.md` section 9, `document.md` op 5 and its rule 1), and the
//! place saved for it (`library.md` section 2.2, `position` and `set_position`).
//!
//! Progress is what Beta 3's `Document::progress` is: the source text before
//! the page's start over the whole book's, inserted content counting zero.
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    document::{DocCursor, DocId, Documents},
    library::{BookRef, Library, PositionWrite},
    snapshot,
};

/// Most sections whose lengths are kept (a book of more has no progress). Each
/// is 4 bytes: 16 KiB at most.
pub const MAX_SECTIONS: usize = 4096;
/// Most millionths of progress.
const WHOLE: u64 = 1_000_000;
/// The least time between two writes of the place while pages are turned, in
/// microseconds (Beta 3's `SAVE_EVERY_MS`); a screen's close writes it anyway.
pub const SAVE_EVERY_US: u64 = 5_000_000;

/// The lengths of a book's sections, in source units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    lengths: Vec<u32>,
    total: u64,
}

impl Progress {
    /// Progress over `lengths`; an empty list or all zeros has none (0).
    #[must_use]
    pub fn new(lengths: Vec<u32>) -> Self {
        let total = lengths.iter().map(|l| u64::from(*l)).sum();
        Self { lengths, total }
    }

    /// The lengths of the sections of document `id`, in bounded pages. `None`
    /// where the book has too many sections or the host does not tell.
    ///
    /// # Errors
    /// `NoMemory`; the host's other refusals are no progress, not errors.
    pub fn read(
        host: &Host<'_>,
        docs: &Documents,
        id: DocId,
        sections: u32,
        (input, out): (&mut [u8], &mut [u8]),
    ) -> Result<Option<Self>, Error> {
        if usize::try_from(sections).map_or(true, |n| n > MAX_SECTIONS) {
            return Ok(None);
        }
        let mut lengths = Vec::new();
        lengths
            .try_reserve(sections as usize)
            .map_err(|_| Error::NoMemory)?;
        let mut cursor = DocCursor::start();
        loop {
            let page = match docs.sections(host, id, cursor, input, out) {
                Ok(page) => page,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(None),
            };
            for row in page.rows() {
                if lengths.len() >= MAX_SECTIONS {
                    return Ok(None);
                }
                // Rows come in order; a gap or a repeat is a list not to trust.
                if usize::try_from(row.index()).ok() != Some(lengths.len()) {
                    return Ok(None);
                }
                lengths.push(row.length());
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => return Ok(Some(Self::new(lengths))),
            }
        }
    }

    /// The progress before `(section, offset)` in millionths, 0 to 1,000,000.
    #[must_use]
    pub fn ppm(&self, section: u32, offset: u32) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let before: u64 = self
            .lengths
            .iter()
            .take(section as usize)
            .map(|l| u64::from(*l))
            .sum();
        let done = (before + u64::from(offset)).min(self.total);
        u32::try_from(done * WHOLE / self.total).unwrap_or(WHOLE as u32)
    }
}

impl Progress {
    /// The place `ppm` millionths into the book: its section and the offset in
    /// it (the reverse of [`Self::ppm`]; past the end, the last character).
    #[must_use]
    pub fn place_at(&self, ppm: u32) -> (u32, u32) {
        let mut left = self.total * u64::from(ppm).min(WHOLE) / WHOLE;
        let mut last = (0, 0);
        for (section, length) in (0u32..).zip(&self.lengths) {
            let length = u64::from(*length);
            if left < length {
                return (section, u32::try_from(left).unwrap_or(u32::MAX));
            }
            left -= length;
            last = (
                section,
                u32::try_from(length.saturating_sub(1)).unwrap_or(u32::MAX),
            );
        }
        last
    }
}

/// The most the progress of a place saved in another conversion may differ from
/// the progress that place has in this one for it to be taken as it is, in
/// millionths (0.2 %: the file keeps a tenth of a percent).
pub const RELOCATE_WITHIN: u32 = 2_000;

/// What the library keeps of a book's place: where it was, if that fits the
/// book, and the progress stored with it (kept when this screen cannot tell
/// the progress itself).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    /// A section and an offset the book has, in the document's conversion (or
    /// one stored without).
    pub at: Option<(u32, u32)>,
    /// The progress stored, in millionths.
    pub ppm: u32,
    /// A place saved in another conversion of the book: its section and
    /// offset mean what they meant there, so it is looked at by
    /// [`relocate`] before it is taken.
    pub other: Option<(u32, u32)>,
}

impl Progress {
    /// Whether the book has `offset` in `section` (the end of a section
    /// counts: a page may start there).
    #[must_use]
    pub fn holds(&self, section: u32, offset: u32) -> bool {
        self.lengths
            .get(section as usize)
            .is_some_and(|length| offset <= *length)
    }
}

/// Where a place saved in another conversion is in this one: the place itself
/// where its progress here is what was stored with it (within
/// [`RELOCATE_WITHIN`]: the text before it is the same), else the place that
/// progress is (Beta 3's own way when a place did not fit), else none, so the
/// book opens at its start. Nothing is written.
#[must_use]
pub fn relocate(
    other: (u32, u32),
    stored_ppm: u32,
    progress: Option<&Progress>,
) -> Option<(u32, u32)> {
    let progress = progress?;
    if progress.total == 0 || stored_ppm == 0 {
        // No progress stored (or none to measure it by): only the start is
        // sure to be where it was.
        return (other == (0, 0)).then_some(other);
    }
    if progress.holds(other.0, other.1)
        && progress.ppm(other.0, other.1).abs_diff(stored_ppm) <= RELOCATE_WITHIN
    {
        return Some(other);
    }
    Some(progress.place_at(stored_ppm))
}

/// The place saved for the book at `path`: its own, or one of another
/// conversion as `other`. A read that fails other than as
/// "no place" is tried once more, as Beta 3 did, before the book opens at its
/// start.
#[must_use]
pub fn saved(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    (sections, conversion): (u32, u64),
    (input, out): (&mut [u8], &mut [u8]),
) -> Stored {
    let Ok(book) = BookRef::path(path) else {
        return Stored::default();
    };
    let mut read = library.position(host, book, input, out);
    if matches!(&read, Err(e) if *e != Error::NotFound) {
        read = library.position(host, book, input, out);
    }
    let Ok(position) = read else {
        return Stored::default();
    };
    let here = (position.section, position.offset);
    let mine = position.conversion.is_none_or(|c| c == conversion);
    Stored {
        at: (mine && position.section < sections).then_some(here),
        ppm: position.progress_ppm.min(1_000_000),
        other: (!mine).then_some(here),
    }
}

/// Most bytes of the title a report carries.
pub const TITLE_MAX: usize = 256;

/// `text` cut at a character boundary to at most [`TITLE_MAX`] bytes.
#[must_use]
pub fn cut_title(text: &str) -> &str {
    let mut end = text.len().min(TITLE_MAX);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// The title a report carries: the book's, else its path (as Beta 3), cut.
#[must_use]
pub fn title_of<'a>(title: &'a str, path: &'a str) -> &'a str {
    cut_title(if title.is_empty() { path } else { title })
}

/// The progress a report carries: the whole once the book was read to the end,
/// also where the reader pages back (Beta 3's `reading()`).
#[must_use]
pub const fn report_ppm(finished: bool, ppm: u32) -> u32 {
    if finished { 1_000_000 } else { ppm }
}

/// Whether a page ending at progress `end_ppm` ends the book: it is read to the
/// end (Beta 3's `at_end`, progress of 0.999).
#[must_use]
pub const fn reaches_end(end_ppm: u32) -> bool {
    end_ppm >= 999_000
}

/// The wall time of the write in seconds since 1970, from the snapshot; 0 where
/// the device has no valid clock (`library.md` section 2.2: never made up).
#[must_use]
pub fn wall_seconds(host: &Host<'_>) -> u64 {
    snapshot::capture_snapshot(host)
        .ok()
        .and_then(|s| s.unix_seconds())
        .and_then(|t| u64::try_from(t).ok())
        .unwrap_or(0)
}

/// Writes the place of the page that starts at `at`.
///
/// # Errors
///
/// The library's refusal (`library.md` section 2.2), or [`Error::Invalid`] for a
/// path that is no book reference.
pub fn save(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    at: (u32, u32),
    (ppm, conversion, updated): (u32, u64, u64),
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<(), Error> {
    let book = BookRef::path(path).map_err(|_| Error::Invalid)?;
    let write = PositionWrite::new(at.0, at.1)
        .progress_ppm(ppm.min(1_000_000))
        .conversion(conversion)
        .updated(updated);
    library
        .set_position(host, book, &write, input, out)
        .map(|_| ())
}

/// What the reader does about a place the library refused to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Nothing said: the next save tries again (a book the library has not
    /// listed yet, or a library busy with something else).
    Retry,
    /// Said with this text.
    Say(&'static str),
}

/// What a refusal of the place (`error`) does while reading, or when the book
/// `closing` (no later save tries again then). Only a lack of memory says the
/// memory may be full; a full card (`NoSpace`, `library.md` section 6: the
/// place is a few lines, so never too large) says so; anything else, also a
/// full table (`Full`), says just that the place was not saved.
#[must_use]
pub const fn place_refused(error: Error, closing: bool) -> Refused {
    match error {
        Error::NotFound | Error::Busy if !closing => Refused::Retry,
        Error::NoMemory => Refused::Say("reader.not-saved"),
        Error::NoSpace => Refused::Say("reader.not-saved-full"),
        _ => Refused::Say("reader.not-saved-plain"),
    }
}

#[cfg(test)]
mod tests;
