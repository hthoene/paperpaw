//! A book's whole texts: title, author, series and the items of its lists (tags and plugin
//! fields), read in bounded parts (`docs/abi/library.md` 2.1, op 4, kinds 0 to 4).
use super::annotation_text::text_reply;
use super::*;

/// Which text of a book to read, and from which byte.
///
/// `title`, `author` and `series` are whole texts; `tag(index)` and `field(index)` pick an item
/// of a list whose length [`Book::tags`] and [`Book::fields`] give. An `index` at or past the
/// count is `NotFound`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookTextQuery {
    kind: u32,
    index: u32,
    offset: u32,
}
impl BookTextQuery {
    const fn new(kind: u32, index: u32) -> Self {
        Self {
            kind,
            index,
            offset: 0,
        }
    }
    /// The book's whole title.
    #[must_use]
    pub const fn title() -> Self {
        Self::new(0, 0)
    }
    /// The book's whole author.
    #[must_use]
    pub const fn author() -> Self {
        Self::new(1, 0)
    }
    /// The book's whole series.
    #[must_use]
    pub const fn series() -> Self {
        Self::new(2, 0)
    }
    /// The tag at `index`, counted from 0.
    #[must_use]
    pub const fn tag(index: u32) -> Self {
        Self::new(3, index)
    }
    /// The plugin field at `index`, as `id=value` (the field's full id, `=`, its value).
    #[must_use]
    pub const fn field(index: u32) -> Self {
        Self::new(4, index)
    }
    /// Continue at the byte offset a preceding part returned.
    #[must_use]
    pub const fn from(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }
}

/// A borrowed UTF-8 part of one book text, at most 1,024 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookTextPart<'a> {
    text: &'a str,
    total: u32,
    next: Option<u32>,
}
impl<'a> BookTextPart<'a> {
    /// This part's text.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }
    /// The whole text's length in UTF-8 bytes.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// The byte offset of the next part, or `None` when the text is complete.
    #[must_use]
    pub const fn next_offset(self) -> Option<u32> {
        self.next
    }
}

impl Library {
    /// Read one of a book's whole texts in parts of at most 1,024 bytes: start at offset 0 and
    /// continue with [`BookTextPart::next_offset`] until it is `None`. Nothing is cut for good
    /// (the catalog rows and [`Library::book`] cut long values; this does not).
    ///
    /// `input` of 1,024 bytes and `out` of 4,096 always suffice. No generation is asked: a
    /// book that changes between parts can answer `NotFound` or a different text, so a caller
    /// that needs a stable text reads it again from 0.
    ///
    /// # Errors
    /// `NotFound` for an unknown book or a list index past the count; `State` for a stale
    /// handle; `Denied` without `uses.library`; `Invalid` for a reply that is not the text
    /// asked for, is cut mid-character or is the wrong length.
    pub fn book_text<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        query: BookTextQuery,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<BookTextPart<'a>, Error> {
        let BookTextQuery {
            kind,
            index,
            offset,
        } = query;
        let request = wire::ServiceLibTextAt {
            kind,
            index,
            offset,
        };
        let n = self.annotation_call(host, schema::OP_TEXT, &[&book.0, &request], input, out)?;
        let (text, total, next) =
            text_reply(out.get(..n).ok_or(Error::Invalid)?, kind, index, offset)?;
        Ok(BookTextPart { text, total, next })
    }
}
