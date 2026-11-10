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

/// The place saved for the book at `path`, if it has one that fits the book:
/// a section the book has, and a conversion that is the open document's (or
/// none was stored).
#[must_use]
pub fn saved(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    (sections, conversion): (u32, u64),
    (input, out): (&mut [u8], &mut [u8]),
) -> Option<(u32, u32)> {
    let book = BookRef::path(path).ok()?;
    let position = library.position(host, book, input, out).ok()?;
    let fits = position.section < sections && position.conversion.is_none_or(|c| c == conversion);
    fits.then_some((position.section, position.offset))
}

/// Writes the place of the page that starts at `at`; whether it was kept.
pub fn save(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    at: (u32, u32),
    (ppm, conversion): (u32, u64),
    (input, out): (&mut [u8], &mut [u8]),
) -> bool {
    let Ok(book) = BookRef::path(path) else {
        return false;
    };
    let write = PositionWrite::new(at.0, at.1)
        .progress_ppm(ppm.min(1_000_000))
        .conversion(conversion);
    library.set_position(host, book, &write, input, out).is_ok()
}

#[cfg(test)]
mod tests;
