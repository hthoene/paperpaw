//! [`Source`] over the library service (`core.library`): the catalog by
//! title, a book's annotations by kind, a book's title. The buffers
//! of the calls are the 4,096 bytes a reply fits; nothing else is allocated
//! but what the scan keeps.
use crate::model::{self, Handle, Kind, Note};
use crate::native_export::Texts;
use crate::native_scan::{Page, Source};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    library::{
        AnnotationCursor, AnnotationKind, AnnotationTextKind, AnnotationTextQuery, BookRef,
        CatalogCursor, Library, Query,
    },
};

/// The most of a note's text, or of its own note, that the export reads whole
/// (the library cuts its previews at 512 bytes). Interface 6 read them
/// whole, from a file of up to 1 MiB; a longer text is cut here at a
/// character's end.
pub const TEXT_LIMIT: usize = 8 * 1024;

/// The size of the buffers handed to the library: the most a reply holds.
pub const BUFFER: usize = 4096;

/// A book of the catalog: its key in the generation of the page it came on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ref {
    key: u32,
    generation: u32,
}

/// The library service, with the buffers of its calls.
#[derive(Debug)]
pub struct HostSource<'a, 'h> {
    host: &'a Host<'h>,
    library: &'a Library,
    input: Vec<u8>,
    out: Vec<u8>,
    /// What the export reads besides: only with [`HostSource::for_export`].
    extra: Option<Extra>,
}

/// The buffers of the whole-text calls (the page's own are in use while its
/// rows are read) and the two texts read whole.
#[derive(Debug)]
struct Extra {
    input: Vec<u8>,
    out: Vec<u8>,
    text: String,
    note: String,
}

impl<'a, 'h> HostSource<'a, 'h> {
    /// A source over `library`.
    ///
    /// # Errors
    /// `NoMemory` when the buffers cannot be had.
    pub fn new(host: &'a Host<'h>, library: &'a Library) -> Result<Self, Error> {
        Ok(Self {
            host,
            library,
            input: buffer()?,
            out: buffer()?,
            extra: None,
        })
    }

    /// A source that also reads the whole texts of the notes
    /// ([`Texts`]); four buffers of [`BUFFER`] bytes and two texts of
    /// [`TEXT_LIMIT`] bytes at most.
    ///
    /// # Errors
    /// `NoMemory` when the buffers cannot be had.
    pub fn for_export(host: &'a Host<'h>, library: &'a Library) -> Result<Self, Error> {
        let mut source = Self::new(host, library)?;
        source.extra = Some(Extra {
            input: buffer()?,
            out: buffer()?,
            text: String::new(),
            note: String::new(),
        });
        Ok(source)
    }
}

fn buffer() -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(BUFFER)
        .map_err(|_| Error::NoMemory)?;
    bytes.resize(BUFFER, 0);
    Ok(bytes)
}

impl Source for HostSource<'_, '_> {
    type Book = Ref;
    type Catalog = CatalogCursor;
    type Notes = AnnotationCursor;

    fn books(
        &mut self,
        cursor: CatalogCursor,
        each: &mut dyn FnMut(Ref) -> Result<(), Error>,
    ) -> Result<Page<CatalogCursor>, Error> {
        let query = Query::new().cursor(cursor);
        let page = self
            .library
            .query(self.host, &query, &mut self.input, &mut self.out)?;
        let end = page.end();
        for row in page.rows() {
            each(Ref {
                key: row?.key(),
                generation: end.generation(),
            })?;
        }
        Ok(Page {
            next: end.next(),
            total: end.total(),
        })
    }

    fn notes(
        &mut self,
        book: Ref,
        cursor: AnnotationCursor,
        each: &mut dyn FnMut(Kind),
    ) -> Result<Page<AnnotationCursor>, Error> {
        let page = self.library.annotations(
            self.host,
            BookRef::catalog(book.key, book.generation)?,
            cursor,
            &mut self.input,
            &mut self.out,
        )?;
        for row in page.rows() {
            each(match row?.kind() {
                AnnotationKind::Highlight => Kind::Highlight,
                AnnotationKind::Bookmark => Kind::Bookmark,
                _ => Kind::Other,
            });
        }
        let end = page.end();
        Ok(Page {
            next: end.next(),
            total: end.total(),
        })
    }

    fn handle(&self, book: Ref) -> Handle {
        Handle {
            key: book.key,
            generation: book.generation,
        }
    }

    fn title(
        &mut self,
        book: Ref,
        put: &mut dyn FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let book = self.library.book(
            self.host,
            BookRef::catalog(book.key, book.generation)?,
            &mut self.input,
            &mut self.out,
        )?;
        put(book.title())
    }
}

/// Reads the whole string `kind` of the annotation `id` into `to`, up to
/// [`TEXT_LIMIT`] bytes.
fn whole(
    library: &Library,
    host: &Host<'_>,
    book: BookRef<'_>,
    (id, kind): (u32, AnnotationTextKind),
    to: &mut String,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<(), Error> {
    to.clear();
    let mut offset = 0u32;
    loop {
        let query = AnnotationTextQuery::new(id, kind).from(offset);
        let part = library.annotation_text(host, book, query, input, out)?;
        let piece = model::cut(part.text(), TEXT_LIMIT.saturating_sub(to.len()));
        to.try_reserve(piece.len()).map_err(|_| Error::NoMemory)?;
        to.push_str(piece);
        if piece.len() < part.text().len() {
            return Ok(());
        }
        match part.next_offset() {
            Some(next) => offset = next,
            None => return Ok(()),
        }
    }
}

impl Texts for HostSource<'_, '_> {
    fn names(
        &mut self,
        book: Ref,
        put: &mut dyn FnMut(&str, &str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let book = self.library.book(
            self.host,
            BookRef::catalog(book.key, book.generation)?,
            &mut self.input,
            &mut self.out,
        )?;
        put(book.title(), book.author())
    }

    fn texts(
        &mut self,
        book: Ref,
        cursor: AnnotationCursor,
        each: &mut dyn FnMut(&Note<'_>) -> Result<(), Error>,
    ) -> Result<Page<AnnotationCursor>, Error> {
        let Some(extra) = self.extra.as_mut() else {
            return Err(Error::State);
        };
        let reference = BookRef::catalog(book.key, book.generation)?;
        let page = self.library.annotations(
            self.host,
            reference,
            cursor,
            &mut self.input,
            &mut self.out,
        )?;
        for row in page.rows() {
            let row = row?;
            let kind = match row.kind() {
                AnnotationKind::Highlight => Kind::Highlight,
                AnnotationKind::Bookmark => Kind::Bookmark,
                _ => Kind::Other,
            };
            let id = row.id();
            let side = (&mut extra.input[..], &mut extra.out[..]);
            let text = if kind != Kind::Other && row.text_cut() {
                whole(
                    self.library,
                    self.host,
                    reference,
                    (id, AnnotationTextKind::Text),
                    &mut extra.text,
                    side,
                )?;
                extra.text.as_str()
            } else {
                row.text()
            };
            let side = (&mut extra.input[..], &mut extra.out[..]);
            let note = if kind == Kind::Highlight && row.note_cut() {
                whole(
                    self.library,
                    self.host,
                    reference,
                    (id, AnnotationTextKind::Note),
                    &mut extra.note,
                    side,
                )?;
                extra.note.as_str()
            } else {
                row.note()
            };
            each(&Note {
                kind,
                percent: u8::try_from(row.percent().min(100)).unwrap_or(100),
                created: i64::try_from(row.created()).unwrap_or(0),
                updated: i64::try_from(row.updated()).unwrap_or(0),
                text,
                note,
            })?;
        }
        let end = page.end();
        Ok(Page {
            next: end.next(),
            total: end.total(),
        })
    }
}
