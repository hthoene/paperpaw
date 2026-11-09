//! A document's metadata (`docs/abi/document.md` 2.2, ops 3 and 4): title, language, series,
//! direction, the counts of authors and subjects, and every text whole, by parts.
use super::*;

/// Highest `writing_mode` the contract names (`document.md` 2.2): 0 horizontal, 1 vertical
/// right to left, 2 vertical left to right, 3 and 4 sideways.
const WRITING_MODE_MAX: u32 = 4;
/// `doc_meta.flags`: bit 0 the title was cut, bit 1 the series was cut.
const FLAGS: u32 = 0b11;

/// What a book's format says about it, borrowed from the caller's reply buffer. The title and
/// the series are cut at their caps (see [`title_cut`](Self::title_cut)); every text is read
/// whole with [`Documents::text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata<'a>(wire::ServiceDocMeta<'a>);

impl<'a> Metadata<'a> {
    fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let row = one(bytes)?
            .decode::<wire::ServiceDocMeta<'a>>()
            .map_err(|_| Error::Invalid)?;
        if row.direction > 1 || row.flags & !FLAGS != 0 {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }
    /// The title, at most 512 bytes (cut when [`title_cut`](Self::title_cut)).
    #[must_use]
    pub const fn title(&self) -> &'a str {
        self.0.title
    }
    /// The book's language as a BCP 47 tag, at most 35 bytes; empty when the format names none.
    #[must_use]
    pub const fn language(&self) -> &'a str {
        self.0.language
    }
    /// The series, at most 512 bytes (cut when [`series_cut`](Self::series_cut)).
    #[must_use]
    pub const fn series(&self) -> &'a str {
        self.0.series
    }
    /// The number in the series as text, at most 32 bytes.
    #[must_use]
    pub const fn series_index(&self) -> &'a str {
        self.0.series_index
    }
    /// Whether the text runs right to left.
    #[must_use]
    pub const fn right_to_left(&self) -> bool {
        self.0.direction == 1
    }
    /// The writing mode as the contract numbers it: 0 horizontal, 1 vertical right to left,
    /// 2 vertical left to right, 3 and 4 sideways. A later value this SDK does not know reads
    /// as 0, never an error, so a newer format still opens (`document.md` 4).
    #[must_use]
    pub const fn writing_mode(&self) -> u32 {
        if self.0.writing_mode > WRITING_MODE_MAX {
            0
        } else {
            self.0.writing_mode
        }
    }
    /// How many sections the book has.
    #[must_use]
    pub const fn sections(&self) -> u32 {
        self.0.sections
    }
    /// How many authors: read each with [`DocTextQuery::author`].
    #[must_use]
    pub const fn authors(&self) -> u32 {
        self.0.authors
    }
    /// How many subjects: read each with [`DocTextQuery::subject`].
    #[must_use]
    pub const fn subjects(&self) -> u32 {
        self.0.subjects
    }
    /// The description's length in bytes (0 for none): read it with
    /// [`DocTextQuery::description`].
    #[must_use]
    pub const fn description_bytes(&self) -> u32 {
        self.0.description_bytes
    }
    /// Whether the title here is cut; read it whole with [`DocTextQuery::title`].
    #[must_use]
    pub const fn title_cut(&self) -> bool {
        self.0.flags & 1 != 0
    }
    /// Whether the series here is cut; read it whole with [`DocTextQuery::series`].
    #[must_use]
    pub const fn series_cut(&self) -> bool {
        self.0.flags & 2 != 0
    }
}

/// Which text of a document to read, and from which byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocTextQuery {
    kind: u32,
    index: u32,
    offset: u32,
}
impl DocTextQuery {
    const fn new(kind: u32, index: u32) -> Self {
        Self {
            kind,
            index,
            offset: 0,
        }
    }
    /// The whole title.
    #[must_use]
    pub const fn title() -> Self {
        Self::new(0, 0)
    }
    /// The author at `index`, below [`Metadata::authors`].
    #[must_use]
    pub const fn author(index: u32) -> Self {
        Self::new(1, index)
    }
    /// The subject at `index`, below [`Metadata::subjects`].
    #[must_use]
    pub const fn subject(index: u32) -> Self {
        Self::new(2, index)
    }
    /// The whole description (its length is [`Metadata::description_bytes`]).
    #[must_use]
    pub const fn description() -> Self {
        Self::new(3, 0)
    }
    /// The whole series.
    #[must_use]
    pub const fn series() -> Self {
        Self::new(4, 0)
    }
    /// Continue at the byte offset a preceding part returned.
    #[must_use]
    pub const fn from(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }
}

/// A borrowed UTF-8 part of one document text, at most 1,024 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocTextPart<'a> {
    text: &'a str,
    total: u32,
    next: Option<u32>,
}
impl<'a> DocTextPart<'a> {
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

impl Documents {
    /// Read a document's metadata (op 3), borrowed from `out`. `input` of 64 bytes and `out`
    /// of 4,096 always suffice.
    ///
    /// # Errors
    /// `Busy` while the document is preparing; `Cancelled` for a document the host closed;
    /// `Invalid` for a malformed reply (a direction above 1 or an unknown flag); otherwise the host's refusals.
    pub fn metadata<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Metadata<'a>, Error> {
        let n = self.call(host, schema::OP_META, &[&document.record()], input, out)?;
        Metadata::decode(out.get(..n).ok_or(Error::Invalid)?)
    }

    /// Read one of a document's texts in parts of at most 1,024 bytes (op 4): start at offset 0
    /// and continue with [`DocTextPart::next_offset`] until it is `None`. Nothing is cut for
    /// good. `input` of 64 bytes and `out` of 4,096 always suffice.
    ///
    /// # Errors
    /// `NotFound` for an author or subject index past its count; `Busy` while the document is
    /// preparing; `Invalid` for a reply that is not the text asked for, is cut mid-character
    /// or is the wrong length; otherwise the host's refusals.
    pub fn text<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        query: DocTextQuery,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DocTextPart<'a>, Error> {
        let DocTextQuery {
            kind,
            index,
            offset,
        } = query;
        let request = wire::ServiceDocTextAt {
            document: document.0,
            kind,
            index,
            offset,
        };
        let n = self.call(host, schema::OP_META_TEXT, &[&request], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceDocText<'a>>()
            .map_err(|_| Error::Invalid)?;
        let remaining = row.total.checked_sub(offset).ok_or(Error::Invalid)?;
        let len = u32::try_from(row.text.len()).map_err(|_| Error::Invalid)?;
        let want = remaining.min(1024);
        let next = offset.checked_add(len).ok_or(Error::Invalid)?;
        if row.kind != kind
            || row.index != index
            || len > want
            || len + 3 < want
            || remaining <= 1024 && len != remaining
        {
            return Err(Error::Invalid);
        }
        Ok(DocTextPart {
            text: row.text,
            total: row.total,
            next: (next < row.total).then_some(next),
        })
    }
}
