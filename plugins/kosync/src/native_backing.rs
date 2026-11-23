//! What the sync asks of the book and the library: the book's KOReader
//! fingerprint (`core.library` op 25 `book_key`), and a place's XPointer and the
//! place of an XPointer (`core.document` ops `xpointer` and `locate`,
//! `document.md` section 2). A service the device does not offer, a book the
//! library does not know answer "unavailable" (`Ok(None)`). Formats without
//! XPointers send progress alone, except a book of whole pages (a comic):
//! its progress is the 1-based page number, as KOReader and Readest send it
//! for a fixed layout. Incoming progress uses section lengths.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::{
    Error, Host,
    document::{DocCursor, DocId, DocPosition, Documents, OpenBook, Preparation},
    library::{BookRef, Library},
};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    text::copy,
};

/// The input and the reply a service call needs; 4,096 bytes always suffice.
const BUFFER: usize = 4096;
/// Fingerprints kept: the open book's and those of the books left whose
/// progress may still go out.
const KEPT_DIGESTS: usize = 6;

/// The two services, opened where the device has them.
#[derive(Debug, Default)]
pub struct Backing {
    library: Option<Library>,
    documents: Option<Documents>,
    /// The fingerprints found, by path, the latest last: the library reads a
    /// book's from the card (about a dozen reads), once, not at every fetch
    /// and push.
    digests: Vec<(String, String)>,
}

impl Backing {
    /// The services the device offers (`None` for each it does not).
    ///
    /// # Errors
    /// `NoMemory`, and what a service says other than that it is not offered.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        let library = optional(Library::open(host))?;
        let documents = match optional(Documents::open(host)) {
            Ok(documents) => documents,
            Err(error) => {
                if let Some(library) = library {
                    library.release(host).ok();
                }
                return Err(error);
            }
        };
        Ok(Self {
            library,
            documents,
            digests: Vec::new(),
        })
    }

    /// Whether a position can be sent at all: the library names the book. Nothing
    /// is declared for a push while it cannot (it would wake the radio for
    /// nothing).
    #[must_use]
    pub const fn can_send(&self) -> bool {
        self.library.is_some()
    }

    /// The KOReader fingerprint of the book at `path`: the 32 hex digits of its
    /// partial MD5 (`library.md` section 6); `None` for a book the library does
    /// not know or no library.
    ///
    /// # Errors
    /// `NoMemory`; `Busy` where the host asks to try again.
    pub fn digest(&mut self, host: &Host<'_>, path: &str) -> Result<Option<String>, Error> {
        if let Some((_, digest)) = self.digests.iter().find(|(p, _)| p == path) {
            return copy(digest).map(Some);
        }
        let Some(library) = &self.library else {
            return Ok(None);
        };
        let Ok(book) = BookRef::path(path) else {
            return Ok(None);
        };
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let digest = match library.book_key(host, book, &mut input, &mut out) {
            Ok(key) if key.fingerprint().is_empty() => return Ok(None),
            Ok(key) => copy(key.fingerprint())?,
            Err(error) => return unavailable(error),
        };
        if self.digests.len() >= KEPT_DIGESTS {
            self.digests.remove(0);
        }
        push(&mut self.digests, (copy(path)?, copy(&digest)?))?;
        Ok(Some(digest))
    }

    /// The XPointer of the place `at` (section, offset) of the book at `path`;
    /// `None` where the book has none (a format without XPointers, a book still
    /// being prepared, no document service).
    ///
    /// # Errors
    /// `NoMemory`; `Busy` where the host asks to try again.
    pub fn xpointer(
        &self,
        host: &Host<'_>,
        path: &str,
        at: (u32, u32),
    ) -> Result<Option<String>, Error> {
        self.with_document(host, path, |documents, id, kind, input, out| {
            if kind == Kind::Pages {
                // The page's number, as KOReader and Readest read a fixed layout's progress.
                let mut buf = [0; 10];
                return copy(digits(at.0.saturating_add(1), &mut buf)).map(Some);
            }
            if kind != Kind::Pointers {
                return Ok(None);
            }
            match documents.xpointer(host, id, DocPosition::new(at.0, at.1), input, out) {
                Ok("") => Ok(None),
                Ok(text) => copy(text).map(Some),
                Err(error) => unavailable(error),
            }
        })
    }

    /// The XPointer's place, or the percentage's place where no pointer resolves.
    ///
    /// # Errors
    /// `NoMemory`; `Busy` where the host asks to try again.
    pub fn locate(
        &self,
        host: &Host<'_>,
        path: &str,
        xpointer: &str,
        progress: f64,
    ) -> Result<Option<(u32, u32)>, Error> {
        self.with_document(host, path, |documents, id, kind, input, out| {
            if kind == Kind::Pointers && !xpointer.is_empty() {
                match documents.locate(host, id, xpointer, input, out) {
                    Ok(at) => return Ok(Some((at.section, at.offset))),
                    Err(Error::Full) => return Err(Error::Busy),
                    Err(error @ (Error::NoMemory | Error::Busy)) => return Err(error),
                    Err(_) => {}
                }
            }
            let Some(lengths) = section_lengths(documents, host, id, input, out)? else {
                return Ok(None);
            };
            // A fixed layout's progress is its 1-based page; its percentage is
            // page / pages, which would land one page late.
            if kind == Kind::Pages
                && let Ok(page) = xpointer.trim().parse::<usize>()
                && (1..=lengths.len()).contains(&page)
            {
                return Ok(u32::try_from(page - 1).ok().map(|section| (section, 0)));
            }
            Ok(at_progress(&lengths, progress))
        })
    }

    /// Opens the book, runs `ask` on it when it is ready, and
    /// closes it again (also when `ask` fails).
    fn with_document<T>(
        &self,
        host: &Host<'_>,
        path: &str,
        ask: impl FnOnce(&Documents, DocId, Kind, &mut [u8], &mut [u8]) -> Result<Option<T>, Error>,
    ) -> Result<Option<T>, Error> {
        let Some(documents) = &self.documents else {
            return Ok(None);
        };
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let (id, usable, kind) = match documents.open_book(
            host,
            &OpenBook::new(path).background(),
            &mut input,
            &mut out,
        ) {
            Ok(state) => {
                let ready = state.preparation() == Preparation::Ready;
                // A book still converting: asked again at the next chance.
                if state.preparation() == Preparation::Preparing {
                    let id = state.id();
                    close(documents, host, id, &mut input, &mut out);
                    return Err(Error::Busy);
                }
                let kind = if state.has_xpointers() {
                    Kind::Pointers
                } else if state.whole_pages() {
                    Kind::Pages
                } else {
                    Kind::Text
                };
                (state.id(), ready, kind)
            }
            Err(error) => return unavailable(error),
        };
        let found = if usable {
            ask(documents, id, kind, &mut input, &mut out)
        } else {
            Ok(None)
        };
        close(documents, host, id, &mut input, &mut out);
        found
    }
}

/// How a book names a place to other devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// KOReader's XPointers.
    Pointers,
    /// Whole pages: the page's number.
    Pages,
    /// Neither: the percentage alone.
    Text,
}

/// A number as decimal text in `buf`.
fn digits(mut n: u32, buf: &mut [u8; 10]) -> &str {
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + u8::try_from(n % 10).unwrap_or(0);
        n /= 10;
        if n == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("0")
}

/// Closes a document; one that cannot be closed is the host's to forget.
fn close(documents: &Documents, host: &Host<'_>, id: DocId, input: &mut [u8], out: &mut [u8]) {
    match documents.close(host, id, input, out) {
        Ok(()) | Err(_) => {}
    }
}

/// `Ok(None)` for a service the device does not offer or the plugin may not use.
fn optional<T>(opened: Result<T, Error>) -> Result<Option<T>, Error> {
    match opened {
        Ok(service) => Ok(Some(service)),
        Err(Error::Unsupported | Error::NotFound | Error::Denied) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What a refused call means: no memory and busy are said as they are, the rest
/// (not found, denied, unsupported, invalid) is "unavailable".
fn unavailable<T>(error: Error) -> Result<Option<T>, Error> {
    match error {
        Error::Full => Err(Error::Busy),
        Error::NoMemory | Error::Busy => Err(error),
        _ => Ok(None),
    }
}

// Like the reader, keep at most 4,096 section lengths (16 KiB).
fn section_lengths(
    documents: &Documents,
    host: &Host<'_>,
    id: DocId,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<Option<Vec<u32>>, Error> {
    const MAX_SECTIONS: usize = 4096;
    let mut lengths = Vec::new();
    let mut cursor = DocCursor::start();
    loop {
        let page = match documents.sections(host, id, cursor, input, out) {
            Ok(page) => page,
            Err(error) => return unavailable(error),
        };
        if page.end().total() > MAX_SECTIONS as u32 {
            return Ok(None);
        }
        lengths
            .try_reserve(page.len())
            .map_err(|_| Error::NoMemory)?;
        for section in page.rows() {
            if lengths.len() >= MAX_SECTIONS
                || usize::try_from(section.index()).ok() != Some(lengths.len())
            {
                return Ok(None);
            }
            lengths.push(section.length());
        }
        match page.end().next() {
            Some(next) => cursor = next,
            None => return Ok(Some(lengths)),
        }
    }
}

/// Beta 3's inverse progress; at the very end, start the last nonempty section.
fn at_progress(lengths: &[u32], progress: f64) -> Option<(u32, u32)> {
    if lengths.is_empty() || !progress.is_finite() {
        return None;
    }
    let total: u64 = lengths.iter().map(|&n| u64::from(n)).sum();
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "4096 u32 lengths fit exactly in f64; progress is finite and clamped"
    )]
    let mut left = (progress.clamp(0.0, 1.0) * total as f64) as u64;
    for (section, &len) in lengths.iter().enumerate() {
        if left < u64::from(len) {
            return Some((u32::try_from(section).ok()?, u32::try_from(left).ok()?));
        }
        left -= u64::from(len);
    }
    let last = lengths.iter().rposition(|&len| len > 0).unwrap_or(0);
    Some((u32::try_from(last).ok()?, 0))
}

#[cfg(test)]
mod tests {
    use super::at_progress;

    #[test]
    fn percentages_match_beta3_at_boundaries_and_empty_sections() {
        let lengths = [0, 100, 0, 300, 0];
        assert_eq!(at_progress(&lengths, 0.0), Some((1, 0)));
        assert_eq!(at_progress(&lengths, 0.25), Some((3, 0)));
        assert_eq!(at_progress(&lengths, 0.5), Some((3, 100)));
        assert_eq!(at_progress(&lengths, 1.0), Some((3, 0)));
        assert_eq!(at_progress(&[0, 0], 0.5), Some((0, 0)));
        assert_eq!(at_progress(&[], 0.5), None);
    }

    #[test]
    fn huge_section_lengths_and_invalid_percentages_do_not_wrap_or_panic() {
        assert_eq!(at_progress(&[u32::MAX, u32::MAX], 0.5), Some((1, 0)));
        assert_eq!(at_progress(&[100], -1.0), Some((0, 0)));
        assert_eq!(at_progress(&[100], 10.0), Some((0, 0)));
        for progress in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(at_progress(&[100], progress), None);
        }
    }
}
