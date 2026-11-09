//! Facet lists (authors, series, tags, first letters, folders) of a library query.
use super::*;

/// Which list of values `facets` returns, each with the number of books that
/// match the query's other filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Facet {
    /// Authors.
    Authors,
    /// Series.
    Series,
    /// Tags.
    Tags,
    /// First letters of the titles.
    Letters,
    /// Folders.
    Folders,
}
impl Facet {
    const fn code(self) -> u32 {
        match self {
            Self::Authors => 0,
            Self::Series => 1,
            Self::Tags => 2,
            Self::Letters => 3,
            Self::Folders => 4,
        }
    }
}

/// One facet value, borrowed from the caller's reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FacetRow<'a>(wire::ServiceLibFacet<'a>);
impl<'a> FacetRow<'a> {
    fn decode(record: RawRecord<'a>) -> Result<Self, Error> {
        let row = record
            .decode::<wire::ServiceLibFacet<'a>>()
            .map_err(|_| Error::Invalid)?;
        Ok(Self(row))
    }
    /// The value, whole (at most 256 UTF-8 bytes).
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.0.text
    }
    /// How many books of the query have this value.
    #[must_use]
    pub const fn count(self) -> u32 {
        self.0.count
    }
    /// The index of its first book in the same query sorted by this facet, for
    /// jumping to it in a [`Library::query`] list with that ordering.
    #[must_use]
    pub const fn first(self) -> u32 {
        self.0.first
    }
}

/// A facet page validated in full before exposing any borrowed rows.
#[derive(Debug)]
pub struct FacetPage<'a>(LibraryPage<'a, FacetRow<'a>>);
impl<'a> FacetPage<'a> {
    /// Total number of values, the catalog generation and the continuation cursor.
    #[must_use]
    pub const fn end(&self) -> LibraryPageEnd {
        self.0.end()
    }
    /// Number of values in this reply.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether this page has no values.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iterate the borrowed values without allocation.
    pub fn rows(&self) -> impl Iterator<Item = Result<FacetRow<'a>, Error>> + 'a {
        self.0.rows()
    }
}
impl Library {
    /// List one facet of the books `query` selects, in bounded pages. The
    /// query's other fields filter; continue with the cursor of the page's
    /// [`FacetPage::end`] through [`Query::cursor`]. A facet list pages with the
    /// catalog generation: a changed catalog needs a restart from
    /// [`CatalogCursor::start`].
    ///
    /// # Errors
    /// State for a stale cursor; Invalid for a malformed page; otherwise local limits or host refusals.
    pub fn facets<'a>(
        &self,
        host: &Host<'_>,
        query: &Query<'_>,
        facet: Facet,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<FacetPage<'a>, Error> {
        let record = query.facet_record(facet.code());
        let n = self.call(host, schema::OP_FACETS, &[&record], input, out)?;
        rows::page(
            &out[..n],
            Some(query.cursor),
            schema::PAGE_MAX_ROWS,
            FacetRow::decode,
        )
        .map(FacetPage)
    }
}
