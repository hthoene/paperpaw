//! Find by letter: the letters of the titles (the `letters` facet, the
//! names screen of `Shelf::Letters`) and the library's list by title opened
//! at the window with a letter's first book. A list cannot start at an
//! index (a cursor is only made by a reply), so the windows are read from the
//! start until the one that holds the index ([`native_model::find_window`]).
use super::*;

/// The `kind` parameter of a list found by letter; its `value` is the letter.
pub(super) const LETTER: &str = "letter";

/// Pages of letters read at most (32 letters each) to find one.
const MOST_PAGES: usize = 8;

impl NativeFiles {
    /// Opens the library's list by title at the first book under `letter`.
    pub(super) fn open_letter(&mut self, host: &Host<'_>, letter: &str) -> Result<(), Error> {
        self.open_list(host, LETTER, letter)
    }

    /// The list opened for `letter`: shown as `instance`'s screen.
    pub(super) fn opened_letter(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        letter: &str,
    ) -> Result<(), Error> {
        let browser = self.read_letter(host, letter)?;
        self.screens.open(instance, View::Books(browser))
    }

    /// The index of the first book under `letter` in the library by title;
    /// none where the host has no such letter or no jump to it.
    fn letter_first(
        host: &Host<'_>,
        library: &Library,
        letter: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Option<usize>, Error> {
        let mut cursor = CatalogCursor::start();
        for _ in 0..MOST_PAGES {
            let page = library.facets(
                host,
                &Shelf::Letters.query(cursor),
                Facet::Letters,
                input,
                out,
            )?;
            for row in page.rows() {
                let row = row?;
                if row.text() == letter {
                    // `u32::MAX` says there is no jump.
                    return Ok(usize::try_from(row.first())
                        .ok()
                        .filter(|_| row.first() != u32::MAX));
                }
            }
            match page.end().next() {
                Some(next) if !page.is_empty() => cursor = next,
                _ => break,
            }
        }
        Ok(None)
    }

    /// The library by title, its window the one holding the first book of
    /// `letter`, the focus on that book. Where the book cannot be found (the
    /// host refuses, the letter is gone) the list starts at its first book.
    pub(super) fn read_letter(&self, host: &Host<'_>, letter: &str) -> Result<Browser, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let selection = Selection {
            filter: Filter::All,
            order: Order::Name,
            scope: Scope::Library,
            fixed: true,
        };
        let found = {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            let first = Self::letter_first(host, library, letter, &mut input, &mut out);
            first.and_then(|first| match first {
                Some(index) => {
                    let mut source = Replies {
                        host,
                        library,
                        selection: &selection,
                        input: &mut input,
                        out: &mut out,
                    };
                    native_model::find_window(&mut source, CatalogCursor::start(), index).map(Some)
                }
                None => Ok(None),
            })
        };
        let (reading, at) = match found {
            Ok(Some(found)) => (Reading::from(found.start, found.starts), Some(found.at)),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Ok(None) | Err(_) => (Reading::from(CatalogCursor::start(), Vec::new()), None),
        };
        let mut browser = self.read_browser(host, selection, reading, MOST_BOOKS, 0)?;
        match at {
            Some(at) => browser.focus_book(at),
            None => browser.focus_first_book(),
        }
        Ok(browser)
    }
}
