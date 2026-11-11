//! The book being read: the one opened last whose file is still on the card,
//! as the library lists it. What the progress and reading widgets show.
//!
//! Read for the catalog's generation (it moves when a book is opened or
//! finished) and kept until it moves.
use crate::native_books::{buffers, with_library};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::Arg,
    library::{BookRef, Library, Query, Sort},
};
use paperpaw_ui_kit::{
    covers::{Covered, Pictures, cover},
    widget::Cx,
};
use paperpaw_ui_kit::{mem, text};

/// Books looked at, at most, to find one whose file is there.
const LOOKED_AT: usize = 8;

/// The book being read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    pub key: u32,
    pub generation: u32,
    /// The card path, set where the library confirmed the file is there.
    pub path: String,
    pub title: String,
    pub author: String,
    pub series: String,
    pub number: String,
    /// Percent read, 0 to 100.
    pub percent: u8,
}

/// The book being read, and the catalog generation it was read for.
#[derive(Debug, Default)]
pub struct Cache {
    kept: Option<(u32, Option<Current>)>,
}

/// The book being read, kept for the catalog's generation.
///
/// Where the library refuses a call the widget shows no book (tried again at
/// the next draw). Only a refused allocation is an error.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn read<'c>(host: &Host<'_>, cache: &'c mut Cache) -> Result<Option<&'c Current>, Error> {
    let generation = with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        Ok(library.state(host, &mut input, &mut out)?.generation())
    });
    let generation = match generation {
        Ok(generation) => generation,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(None),
    };
    if cache
        .kept
        .as_ref()
        .is_none_or(|(kept, _)| *kept != generation)
    {
        match find(host) {
            Ok(found) => cache.kept = Some((generation, found)),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        }
    }
    Ok(cache.kept.as_ref().and_then(|(_, found)| found.as_ref()))
}

fn find(host: &Host<'_>) -> Result<Option<Current>, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let query = Query::new().sort(Sort::RecentlyOpened);
        let page = library.query(host, &query, &mut input, &mut out)?;
        let generation = page.end().generation();
        let mut opened = Vec::new();
        for row in page.rows().take(LOOKED_AT) {
            let row = row?;
            if !row.opened() {
                break;
            }
            mem::push(
                &mut opened,
                Current {
                    key: row.key(),
                    generation,
                    path: String::new(),
                    title: text::copy(row.title())?,
                    author: text::copy(row.author())?,
                    series: text::copy(row.series())?,
                    number: text::copy(row.number())?,
                    percent: row.progress().unwrap_or(0),
                },
            )?;
        }
        first_present(host, library, opened)
    })
}

/// The first of `books` whose file is on the card.
fn first_present(
    host: &Host<'_>,
    library: &Library,
    books: Vec<Current>,
) -> Result<Option<Current>, Error> {
    let (mut ask, mut answer) = (mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?);
    for mut book in books {
        let reference = BookRef::catalog(book.key, book.generation)?;
        match library.book(host, reference, &mut ask, &mut answer) {
            Ok(found) if found.missing() => {}
            Err(Error::NotFound) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Ok(found) => {
                book.path = text::copy(found.path())?;
                return Ok(Some(book));
            }
            _ => return Ok(Some(book)),
        }
    }
    Ok(None)
}

/// What the widget shows, to tell when it changed: the catalog's generation and
/// its books, never 0 (a book opened or finished moves it).
///
/// # Errors
/// The host's refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    crate::native_numbers::shows(host, widget)
}

/// `percent` as the widget writes it: "42 %".
///
/// # Errors
/// Formatting refusals and [`Error::NoMemory`].
pub fn percent_text(cx: &mut Cx<'_>, percent: u8) -> Result<String, Error> {
    cx.format(
        "widgets.books.percent",
        &[Arg::Int {
            name: "percent",
            value: i64::from(percent),
        }],
    )
}

/// The cover of the book, if the widget shows one: what the host has of it.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn covered(cx: &Cx<'_>, book: &Current, pictures: &mut Pictures) -> Result<Covered, Error> {
    if !cx.settings.shows("cover")? {
        return Ok(Covered::None);
    }
    let host = cx.host;
    let found = with_library(host, |library| {
        cover(host, library, (book.key, book.generation))
    })?;
    if found == Covered::Busy {
        pictures.busy = true;
    }
    Ok(found)
}

/// The width and height of a cover the host has ready.
#[must_use]
pub fn shape_of(covered: Covered) -> Option<(u32, u32)> {
    match covered {
        Covered::Ready(ready) => Some((ready.width, ready.height)),
        _ => None,
    }
}
