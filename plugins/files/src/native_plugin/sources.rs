use super::*;
use crate::native_covers::Slot;

/// The replies of the host's library query for one selection.
pub(super) struct Replies<'a, 'h> {
    pub(super) host: &'a Host<'h>,
    pub(super) library: &'a Library,
    pub(super) selection: &'a Selection,
    pub(super) input: &'a mut [u8],
    pub(super) out: &'a mut [u8],
}

impl Source<CatalogCursor> for Replies<'_, '_> {
    fn read(&mut self, from: CatalogCursor) -> Result<Replied<CatalogCursor>, Error> {
        if self.selection.scope == Scope::Next {
            return self.read_next();
        }
        let query = self.selection.query(from)?;
        let page = self
            .library
            .query(self.host, &query, self.input, self.out)?;
        let mut books = Vec::new();
        for entry in page.rows() {
            push(&mut books, BookLine::from_entry(entry?)?)?;
        }
        let end = page.end();
        Ok(Replied {
            books,
            generation: end.generation(),
            total: end.total(),
            next: end.next(),
        })
    }
}

impl Replies<'_, '_> {
    /// The next book of each series begun: all of them in one reply.
    fn read_next(&mut self) -> Result<Replied<CatalogCursor>, Error> {
        let mut source = NextHost {
            host: self.host,
            library: self.library,
            input: &mut *self.input,
            out: &mut *self.out,
            generation: 0,
        };
        let books = collect_next(&mut source, CatalogCursor::start(), MOST_SERIES)?;
        let total = u32::try_from(books.len()).unwrap_or(u32::MAX);
        Ok(Replied {
            books,
            generation: source.generation,
            total,
            next: None,
        })
    }
}

/// The series begun and their next books, from the host's library service.
/// A catalog that changes while they are read is `Error::State`: read again.
pub(super) struct NextHost<'a, 'h> {
    pub(super) host: &'a Host<'h>,
    pub(super) library: &'a Library,
    pub(super) input: &'a mut [u8],
    pub(super) out: &'a mut [u8],
    pub(super) generation: u32,
}

impl NextHost<'_, '_> {
    /// Notes the generation a reply was read in; a second one is a change.
    fn note(&mut self, generation: u32) -> Result<(), Error> {
        match self.generation {
            0 => self.generation = generation,
            seen if seen != generation => return Err(Error::State),
            _ => {}
        }
        Ok(())
    }
}

pub(super) const fn show(begun: Begun) -> Show {
    match begun {
        Begun::Reading => Show::Reading,
        Begun::Finished => Show::Finished,
    }
}

impl SeriesSource<CatalogCursor> for NextHost<'_, '_> {
    fn names(
        &mut self,
        begun: Begun,
        from: CatalogCursor,
    ) -> Result<SeriesNames<CatalogCursor>, Error> {
        let query = Query::new().show(show(begun)).cursor(from);
        let page = self
            .library
            .facets(self.host, &query, Facet::Series, self.input, self.out)?;
        let end = page.end();
        let mut names = Vec::new();
        for row in page.rows() {
            push(&mut names, copy(row?.text())?)?;
        }
        self.note(end.generation())?;
        Ok(SeriesNames {
            names,
            next: end.next(),
        })
    }

    fn next_of(&mut self, begun: Begun, series: &str) -> Result<Option<BookLine>, Error> {
        // Any begun book of the series: the host answers with the series'
        // lowest unread book (`library.md`, op 6).
        let query = Query::new().show(show(begun)).series(series)?;
        let page = self
            .library
            .query(self.host, &query, self.input, self.out)?;
        let generation = page.end().generation();
        let key = match page.rows().next() {
            Some(row) => row?.key(),
            None => return Ok(None),
        };
        self.note(generation)?;
        let last = BookRef::catalog(key, generation)?;
        match self
            .library
            .next_in_series(self.host, last, self.input, self.out)
        {
            Ok(entry) => Ok(Some(BookLine::from_entry(entry)?)),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The names of one shelf from the host's `facets`.
pub(super) struct NameReplies<'a, 'h> {
    pub(super) host: &'a Host<'h>,
    pub(super) library: &'a Library,
    pub(super) shelf: Shelf,
    pub(super) input: &'a mut [u8],
    pub(super) out: &'a mut [u8],
}

impl Source<CatalogCursor, NameLine> for NameReplies<'_, '_> {
    fn read(&mut self, from: CatalogCursor) -> Result<Replied<CatalogCursor, NameLine>, Error> {
        let query = Query::new().cursor(from);
        let page =
            self.library
                .facets(self.host, &query, self.shelf.facet(), self.input, self.out)?;
        let mut books = Vec::new();
        for row in page.rows() {
            let row = row?;
            push(&mut books, {
                let (text, cut) = native_shelves::short(row.text(), native_shelves::MOST_NAME);
                NameLine {
                    text: copy(text)?,
                    cut,
                    count: row.count(),
                }
            })?;
        }
        let end = page.end();
        Ok(Replied {
            books,
            generation: end.generation(),
            total: end.total(),
            next: end.next(),
        })
    }
}

/// The catalog's words for the model, in the reader's language.
pub(super) struct Speaker<'h, 'a> {
    pub(super) host: &'h Host<'a>,
    pub(super) messages: Messages,
}

impl Say for Speaker<'_, '_> {
    fn say(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    fn count(&mut self, key: &str, count: i64) -> Result<String, Error> {
        self.messages.format(self.host, key, &[Arg::Count(count)])
    }

    fn with(&mut self, key: &str, name: &str, value: &str) -> Result<String, Error> {
        self.messages
            .format(self.host, key, &[Arg::Text { name, value }])
    }
}

/// The books whose covers are asked for.
pub(super) enum Keys<'a> {
    /// A window of books: the cover of the book at an index is its own.
    Books(&'a [BookLine]),
    /// One book, the one of the information: its catalog key.
    One(u32),
}

/// Asks the host for covers while a page is drawn: one buffer pair for the
/// page, taken when the first cover is asked for.
pub(super) struct Asker<'a, 'h> {
    host: &'a Host<'h>,
    library: Option<&'a Library>,
    keys: Keys<'a>,
    generation: u32,
    buffers: Option<(Vec<u8>, Vec<u8>)>,
}

impl<'a, 'h> Asker<'a, 'h> {
    pub(super) const fn new(
        host: &'a Host<'h>,
        library: Option<&'a Library>,
        keys: Keys<'a>,
        generation: u32,
    ) -> Self {
        Self {
            host,
            library,
            keys,
            generation,
            buffers: None,
        }
    }

    fn key(&self, at: usize) -> Option<u32> {
        match self.keys {
            Keys::Books(books) => books.get(at).map(|book| book.key),
            Keys::One(key) => (at == 0).then_some(key),
        }
    }
}

/// The ask for a cover is `input` of 1,024 bytes and `out` of 4,096 (`library.md`).
const COVER_INPUT: usize = 1024;
const COVER_OUT: usize = 4096;

impl crate::native_draw::Covering for Asker<'_, '_> {
    fn ask(&mut self, at: usize) -> Slot {
        let (Some(library), Some(key)) = (self.library, self.key(at)) else {
            return Slot::Missing;
        };
        let Ok(reference) = BookRef::catalog(key, self.generation) else {
            return Slot::Missing;
        };
        if self.buffers.is_none() {
            let (Ok(input), Ok(out)) = (buffer(COVER_INPUT), buffer(COVER_OUT)) else {
                // No memory now: asked again by a later frame.
                return Slot::Idle;
            };
            self.buffers = Some((input, out));
        }
        let Some((input, out)) = self.buffers.as_mut() else {
            return Slot::Idle;
        };
        match library.cover(self.host, reference, 0, input, out) {
            Ok(cover) => Slot::Ready(cover.image()),
            // Being made: asked again when the host says one is ready.
            Err(Error::Busy) => Slot::Busy,
            Err(Error::NoMemory) => Slot::Idle,
            // None, or none can be had: the outline shows.
            Err(_) => Slot::Missing,
        }
    }
}
