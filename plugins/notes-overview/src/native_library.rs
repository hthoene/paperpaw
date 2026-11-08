//! [`Source`] over the library service (`core.library`): the catalog by
//! title, a book's annotations by kind, a book's path and title. The buffers
//! of the calls are the 4,096 bytes a reply fits; nothing else is allocated
//! but what the scan keeps.
use crate::model::Kind;
use crate::native_scan::{Page, Source};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    library::{AnnotationCursor, AnnotationKind, BookRef, CatalogCursor, Library, Query},
};

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
        })
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

    fn describe(
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
        put(book.path(), book.title())
    }
}
