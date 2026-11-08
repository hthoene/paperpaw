//! The export of every note to `My Clippings.txt`, over the library service:
//! the catalog is passed a page at a time, and each book's notes are read a
//! page at a time and written as they come, so the export of any number of
//! books needs the page in hand and one note, however long the library is.
//!
//! Written over [`Texts`], which the library service fills in
//! ([`crate::native_library::ExportSource`]) and the tests with a scripted
//! library, and over a [`Sink`], which the staged file fills in
//! ([`crate::native_staged`]). The library is read as a whole: a page that
//! goes stale (a change to the catalog or to a book's notes under the export)
//! ends it with [`Error::State`], and the caller makes the file again from the
//! start, [`RESTARTS`] times at most. A reply that does not add up (more rows
//! than the total it names, a next page that brings nothing) is
//! [`Error::Invalid`]: the export never loops on a host that does not end.
use crate::model::{self, Kind, Note, Sink};
use crate::native_scan::{Page, Source};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

/// How often the export is made again from its start after a stale page,
/// before it gives up with [`Error::State`].
pub const RESTARTS: usize = 3;

/// What an export made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exported {
    /// How many entries the file has.
    pub notes: u32,
    /// How many books they come from.
    pub books: u32,
}

/// What the export asks of the library beside what the scan does.
pub trait Texts: Source {
    /// Calls `put` with the title and the author of `book`.
    ///
    /// # Errors
    /// `NotFound` for a book that is gone.
    fn names(
        &mut self,
        book: Self::Book,
        put: &mut dyn FnMut(&str, &str) -> Result<(), Error>,
    ) -> Result<(), Error>;

    /// One page of `book`'s notes from `cursor`: `each` is called with every
    /// note of it in full (a text the library cut is read whole, up to
    /// [`crate::native_library::TEXT_LIMIT`] bytes).
    ///
    /// # Errors
    /// `State` for a stale cursor; `NotFound` for a book that is gone; the
    /// error of `each`.
    fn texts(
        &mut self,
        book: Self::Book,
        cursor: Self::Notes,
        each: &mut dyn FnMut(&Note<'_>) -> Result<(), Error>,
    ) -> Result<Page<Self::Notes>, Error>;
}

/// Writes the byte order mark and then the entries of every book's notes to
/// `out`, in the library's order. `local` turns seconds since 1970 (UTC) into
/// local ones.
///
/// # Errors
/// `State` when the library changed under the export, `Invalid` for a reply
/// that does not add up, `NoMemory`; the sink's and the host's refusals.
/// What was written is of no use then.
pub fn export<S: Texts>(
    source: &mut S,
    out: &mut dyn Sink,
    local: &dyn Fn(i64) -> i64,
) -> Result<Exported, Error> {
    model::start(out)?;
    let mut done = Exported::default();
    let mut cursor = S::Catalog::default();
    let mut seen = 0u64;
    loop {
        let mut page: Vec<S::Book> = Vec::new();
        let end = source.books(cursor, &mut |book| {
            page.try_reserve(1).map_err(|_| Error::NoMemory)?;
            page.push(book);
            Ok(())
        })?;
        seen += page.len() as u64;
        if seen > u64::from(end.total) || end.next.is_some() && page.is_empty() {
            return Err(Error::Invalid);
        }
        for book in page {
            match one_book(source, out, local, book) {
                Ok(0) => {}
                Ok(notes) => {
                    done.notes = done.notes.saturating_add(notes);
                    done.books += 1;
                }
                // A book that is gone while the export runs is left out.
                Err(Error::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        match end.next {
            Some(next) => cursor = next,
            None => return Ok(done),
        }
    }
}

/// Writes the entries of one book; how many.
fn one_book<S: Texts>(
    source: &mut S,
    out: &mut dyn Sink,
    local: &dyn Fn(i64) -> i64,
    book: S::Book,
) -> Result<u32, Error> {
    let (mut title, mut author) = (String::new(), String::new());
    source.names(book, &mut |t, a| {
        title = model::copy(t)?;
        author = model::copy(a)?;
        Ok(())
    })?;
    let mut count = 0u32;
    let mut cursor = S::Notes::default();
    let mut seen = 0u64;
    loop {
        let mut rows = 0u64;
        let end = source.texts(book, cursor, &mut |note| {
            rows += 1;
            if note.kind == Kind::Other {
                return Ok(());
            }
            count = count.saturating_add(model::put_book(
                out,
                local,
                &title,
                &author,
                core::slice::from_ref(note),
            )?);
            Ok(())
        })?;
        seen += rows;
        if seen > u64::from(end.total) || end.next.is_some() && rows == 0 {
            return Err(Error::Invalid);
        }
        match end.next {
            Some(next) => cursor = next,
            None => return Ok(count),
        }
    }
}
