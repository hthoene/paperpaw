//! Which books have notes, over the library service and without opening a
//! book: the catalog is passed one page at a time, and each book's
//! annotations are paged just far enough to count them by kind. Only the
//! books of the list (200 at most, [`MOST_BOOKS`]) are kept, however many
//! books there are; a library of thousands costs host calls, not memory.
//!
//! The scan is written over [`Source`], which the library service fills in
//! ([`crate::native_library`]) and the tests with a scripted library. A page
//! that goes stale (the catalog or a book's notes changed under the scan) is
//! read again from its start, [`RESTARTS`] times at most, then the scan
//! says [`Error::State`]. A reply that does not add up (more rows than the
//! total it names, a next page that brings nothing) is [`Error::Invalid`]:
//! the scan never loops on a host that does not end.
use crate::model::{Kind, Listing, MOST_BOOKS};
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

/// How often a stale page restarts its reading before the scan gives up.
pub const RESTARTS: usize = 3;

/// Where a page stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page<C> {
    /// The continuation, or `None` at the last page.
    pub next: Option<C>,
    /// How many rows the whole list has.
    pub total: u32,
}

/// What the scan asks of the library.
pub trait Source {
    /// A book of the catalog, valid for one pass.
    type Book: Copy;
    /// A position in the catalog.
    type Catalog: Copy + Default;
    /// A position in one book's notes.
    type Notes: Copy + Default;

    /// One page of the catalog from `cursor`: `each` is called with every
    /// book of it.
    ///
    /// # Errors
    /// `State` for a stale cursor; the host's other refusals.
    fn books(
        &mut self,
        cursor: Self::Catalog,
        each: &mut dyn FnMut(Self::Book) -> Result<(), Error>,
    ) -> Result<Page<Self::Catalog>, Error>;

    /// One page of `book`'s notes from `cursor`: `each` is called with the
    /// kind of every note of it.
    ///
    /// # Errors
    /// `State` for a stale cursor; `NotFound` for a book that is gone.
    fn notes(
        &mut self,
        book: Self::Book,
        cursor: Self::Notes,
        each: &mut dyn FnMut(Kind),
    ) -> Result<Page<Self::Notes>, Error>;

    /// Calls `put` with the path and the title of `book`.
    ///
    /// # Errors
    /// `NotFound` for a book that is gone.
    fn describe(
        &mut self,
        book: Self::Book,
        put: &mut dyn FnMut(&str, &str) -> Result<(), Error>,
    ) -> Result<(), Error>;
}

/// The books with notes, by title: the library passed once.
///
/// A book that is gone while the scan runs is left out.
///
/// # Errors
/// `State` when the library keeps changing, `Invalid` for a reply that does
/// not add up, `NoMemory`; the host's other refusals.
pub fn scan<S: Source>(source: &mut S) -> Result<Listing, Error> {
    let mut tries = 0;
    loop {
        match scan_once(source) {
            Err(Error::State) if tries < RESTARTS => tries += 1,
            other => return other,
        }
    }
}

fn scan_once<S: Source>(source: &mut S) -> Result<Listing, Error> {
    let mut listing = Listing::new();
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
            noted(source, &mut listing, book)?;
        }
        match end.next {
            Some(next) => cursor = next,
            None => break,
        }
    }
    listing.sort();
    Ok(listing)
}

/// Adds `book` to the list if it has notes. Once the list is full only the
/// existence of one more book with notes matters (it sets `more`), so its
/// notes are not counted.
fn noted<S: Source>(source: &mut S, listing: &mut Listing, book: S::Book) -> Result<(), Error> {
    let full = listing.books().len() >= MOST_BOOKS;
    let (highlights, bookmarks) = match counted(source, book, full) {
        Ok(counts) => counts,
        Err(Error::NotFound) => return Ok(()),
        Err(error) => return Err(error),
    };
    if highlights == 0 && bookmarks == 0 {
        return Ok(());
    }
    if full {
        return listing.add("", "", highlights, bookmarks);
    }
    match source.describe(book, &mut |path, title| {
        listing.add(path, title, highlights, bookmarks)
    }) {
        Err(Error::NotFound) => Ok(()),
        other => other,
    }
}

/// How many highlights and bookmarks `book` has; with `any`, it stops at the
/// first of either (the count is then 1 of that kind).
fn counted<S: Source>(source: &mut S, book: S::Book, any: bool) -> Result<(u16, u16), Error> {
    let mut tries = 0;
    'again: loop {
        let (mut highlights, mut bookmarks) = (0u16, 0u16);
        let mut cursor = S::Notes::default();
        let mut seen = 0u64;
        loop {
            let mut rows = 0u64;
            let end = source.notes(book, cursor, &mut |kind| {
                rows += 1;
                match kind {
                    Kind::Highlight => highlights = highlights.saturating_add(1),
                    Kind::Bookmark => bookmarks = bookmarks.saturating_add(1),
                    Kind::Other => {}
                }
            });
            let end = match end {
                Err(Error::State) if tries < RESTARTS => {
                    tries += 1;
                    continue 'again;
                }
                other => other?,
            };
            seen += rows;
            if seen > u64::from(end.total) || end.next.is_some() && rows == 0 {
                return Err(Error::Invalid);
            }
            if any && highlights + bookmarks > 0 {
                return Ok((highlights, bookmarks));
            }
            match end.next {
                Some(next) => cursor = next,
                None => return Ok((highlights, bookmarks)),
            }
        }
    }
}
