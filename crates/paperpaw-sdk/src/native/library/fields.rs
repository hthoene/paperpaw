//! Book-field paging uses the book's field generation, never its catalog generation.
use super::*;

/// A page position within one book's field list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldCursor(CatalogCursor);
impl FieldCursor {
    /// Start the field list again, without a retained generation.
    #[must_use]
    pub const fn start() -> Self {
        Self(CatalogCursor::start())
    }
}
/// One complete field, borrowed from the caller's reply buffer.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a>(wire::ServiceLibField<'a>);
impl<'a> Field<'a> {
    fn decode(record: Record<'a>) -> Result<Self, Error> {
        let Record::ServiceLibField(row) = record else {
            return Err(Error::Invalid);
        };
        super::super::ui::navigation::full_id(row.id)?;
        Ok(Self(row))
    }
    /// The owning plugin's full field id.
    #[must_use]
    pub const fn id(self) -> &'a str {
        self.0.id
    }
    /// Title translated by the host.
    #[must_use]
    pub const fn title(self) -> &'a str {
        self.0.title
    }
    /// Complete value, at most 1,024 UTF-8 bytes.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.0.value
    }
}
/// Validated metadata for one field page.
#[derive(Clone, Copy, Debug)]
pub struct FieldPageEnd(LibraryPageEnd);
impl FieldPageEnd {
    /// Number of fields of this book, at most 64.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.0.total()
    }
    /// This book's field generation, independent of the catalog generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.0.generation()
    }
    /// Resume the same book's list, or none after its last page.
    #[must_use]
    pub fn next(self) -> Option<FieldCursor> {
        self.0.next().map(FieldCursor)
    }
}
/// A field page validated in full before exposing any borrowed rows.
#[derive(Debug)]
pub struct FieldPage<'a>(LibraryPage<'a, Field<'a>>);
impl<'a> FieldPage<'a> {
    /// Terminal count, field generation and continuation cursor.
    #[must_use]
    pub const fn end(&self) -> FieldPageEnd {
        FieldPageEnd(self.0.end())
    }
    /// Number of fields in this reply.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether this page has no fields.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iterate whole borrowed field rows without allocation.
    pub fn rows(&self) -> impl Iterator<Item = Result<Field<'a>, Error>> + 'a {
        self.0.rows()
    }
}
impl Library {
    /// Read a book's complete field list in bounded pages. Keep the catalog
    /// generation in `book` and the field generation in `cursor` separate.
    /// A changed field generation requires restarting from `FieldCursor::start`.
    ///
    /// # Errors
    /// State for stale field generations; Invalid for malformed pages or more
    /// than 64 total fields; otherwise local limits or host refusals.
    pub fn fields<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        cursor: FieldCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<FieldPage<'a>, Error> {
        let request = Record::ServiceLibFieldPage(wire::ServiceLibFieldPage {
            cursor: cursor.0.offset,
            generation: cursor.0.generation,
        });
        let n = self.call(
            host,
            schema::OP_FIELDS,
            &[book.record(), request],
            input,
            out,
        )?;
        let page = rows::page(
            &out[..n],
            Some(cursor.0),
            schema::PAGE_MAX_ROWS,
            Field::decode,
        )?;
        if page.end().total() > 64 {
            return Err(Error::Invalid);
        }
        let mut previous = None;
        for field in page.rows() {
            let field = field?;
            let key = field.id().split_once('.').ok_or(Error::Invalid)?;
            if previous.is_some_and(|earlier| earlier >= key) {
                return Err(Error::Invalid);
            }
            previous = Some(key);
        }
        Ok(FieldPage(page))
    }
}
