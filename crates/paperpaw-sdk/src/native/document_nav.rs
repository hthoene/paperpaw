//! Navigation of an open document (`docs/abi/document.md` 2.3): the table of
//! contents (ops 7 and 8) and the links of a section (ops 12 and 13).
use super::*;

/// Where a page of entries continues; start with [`DocCursor::start`] and
/// continue only with the cursor a page returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocCursor {
    offset: u32,
    generation: u32,
}
impl DocCursor {
    /// The first page.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
}

/// How a page of entries ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocPageEnd {
    total: u32,
    next: Option<DocCursor>,
}
impl DocPageEnd {
    /// The entries of the whole list.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// Where the next page starts, or `None` after the last page.
    #[must_use]
    pub const fn next(self) -> Option<DocCursor> {
        self.next
    }
}

type Decode<'a, T> = fn(RawRecord<'a>) -> Result<T, Error>;

/// A page of table-of-contents entries or links, validated in full before any
/// row is exposed, borrowing the caller's reply buffer.
pub struct DocPage<'a, T> {
    bytes: &'a [u8],
    count: usize,
    end: DocPageEnd,
    decode: Decode<'a, T>,
}
impl<T> core::fmt::Debug for DocPage<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DocPage")
            .field("count", &self.count)
            .field("end", &self.end)
            .finish_non_exhaustive()
    }
}
impl<'a, T> DocPage<'a, T> {
    fn decode(bytes: &'a [u8], cursor: DocCursor, decode: Decode<'a, T>) -> Result<Self, Error> {
        let mut rest = bytes;
        let mut count = 0usize;
        let end = loop {
            let (record, n) = RawRecord::prefix(rest).map_err(|_| Error::Invalid)?;
            rest = rest.get(n..).ok_or(Error::Invalid)?;
            if record.tag() == wire::ServiceEnd::TAG {
                break record
                    .decode::<wire::ServiceEnd>()
                    .map_err(|_| Error::Invalid)?;
            }
            count += 1;
            if count > schema::PAGE_MAX_ROWS {
                return Err(Error::Invalid);
            }
            decode(record)?;
        };
        if !rest.is_empty() || end.generation == 0 {
            return Err(Error::Invalid);
        }
        if cursor.generation != 0 && cursor.generation != end.generation {
            return Err(Error::State);
        }
        let seen = u32::try_from(count).map_err(|_| Error::Invalid)?;
        let expected = cursor.offset.checked_add(seen).ok_or(Error::Invalid)?;
        let last = end.flags & 1 != 0;
        if end.flags > 1
            || end.next != expected
            || end.next > end.total
            || last != (end.next == end.total)
            || (!last && count == 0)
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            bytes,
            count,
            end: DocPageEnd {
                total: end.total,
                next: (!last).then_some(DocCursor {
                    offset: end.next,
                    generation: end.generation,
                }),
            },
            decode,
        })
    }
    /// Where this page ends.
    #[must_use]
    pub const fn end(&self) -> DocPageEnd {
        self.end
    }
    /// Number of rows in this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether this page has no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Iterate the borrowed rows without allocation.
    pub fn rows(&self) -> impl Iterator<Item = T> + use<'a, T> {
        let (mut bytes, mut left, decode) = (self.bytes, self.count, self.decode);
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let (record, n) = RawRecord::prefix(bytes).ok()?;
            bytes = bytes.get(n..)?;
            decode(record).ok()
        })
    }
}

/// One entry of the table of contents, borrowed from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TocEntry<'a>(wire::ServiceDocToc<'a>);
impl<'a> TocEntry<'a> {
    fn decode(record: RawRecord<'a>) -> Result<Self, Error> {
        let row = record
            .decode::<wire::ServiceDocToc<'a>>()
            .map_err(|_| Error::Invalid)?;
        // Levels run 0 to 15.
        if row.level > 15 {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }
    /// The entry's number in the list, the argument of
    /// [`Documents::toc_target`].
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.0.index
    }
    /// How deep the entry sits, 0 to 15.
    #[must_use]
    pub const fn level(&self) -> u32 {
        self.0.level
    }
    /// Where the entry points, which may be approximate (see
    /// [`approximate`](Self::approximate)).
    #[must_use]
    pub const fn position(&self) -> DocPosition {
        DocPosition::new(self.0.section, self.0.offset)
    }
    /// The position is approximate: [`Documents::toc_target`] finds the exact one.
    #[must_use]
    pub const fn approximate(&self) -> bool {
        self.0.flags & 1 != 0
    }
    /// The title was cut at a character boundary.
    #[must_use]
    pub const fn title_cut(&self) -> bool {
        self.0.flags & 2 != 0
    }
    /// The entry's title, at most 512 bytes.
    #[must_use]
    pub const fn title(&self) -> &'a str {
        self.0.title
    }
}

/// What a link points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkKind {
    /// A place in the book ([`Documents::link_target`] resolves it).
    Internal,
    /// An address outside the book.
    External,
    /// A footnote or note: its text is at the target.
    Footnote,
    /// A kind a later host adds, by its number. Once a later SDK names that
    /// kind, its `Other(n)` arm stops matching: match the named kinds first and
    /// treat `Other` as "something else".
    Other(u32),
}

/// One link of a section, borrowed from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Link<'a>(wire::ServiceDocLink<'a>);
impl<'a> Link<'a> {
    fn decode(record: RawRecord<'a>) -> Result<Self, Error> {
        let row = record
            .decode::<wire::ServiceDocLink<'a>>()
            .map_err(|_| Error::Invalid)?;
        if row.end < row.start {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }
    /// The first source offset of the link's text.
    #[must_use]
    pub const fn start(&self) -> u32 {
        self.0.start
    }
    /// The source offset after the link's text.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.0.end
    }
    /// What it points at.
    #[must_use]
    pub const fn kind(&self) -> LinkKind {
        match self.0.kind {
            0 => LinkKind::Internal,
            1 => LinkKind::External,
            2 => LinkKind::Footnote,
            other => LinkKind::Other(other),
        }
    }
    /// Where it points, at most 1,024 bytes.
    #[must_use]
    pub const fn href(&self) -> &'a str {
        self.0.href
    }
}

/// A range of source offsets in one section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocSpan {
    section: u32,
    from: u32,
    to: u32,
}
impl DocSpan {
    /// The offsets `range` of `section`.
    #[must_use]
    pub const fn new(section: u32, range: core::ops::Range<u32>) -> Self {
        Self {
            section,
            from: range.start,
            to: range.end,
        }
    }
}

/// One section of a document: its number and its length for progress. Unknown
/// row flags are ignored, so a later host may add some.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SectionEntry(wire::ServiceDocSection);
impl SectionEntry {
    fn decode(record: RawRecord<'_>) -> Result<Self, Error> {
        record
            .decode::<wire::ServiceDocSection>()
            .map(Self)
            .map_err(|_| Error::Invalid)
    }
    /// The section's number, as [`DocPosition::section`] names it.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.0.index
    }
    /// The section's length in source units, for progress. Add the lengths of
    /// several sections in `u64`: a `u32` sum can wrap.
    #[must_use]
    pub const fn length(&self) -> u32 {
        self.0.length
    }
    /// The section is one whole-page picture.
    #[must_use]
    pub const fn whole_page_picture(&self) -> bool {
        self.0.flags & 1 != 0
    }
}

impl Documents {
    /// List the sections (op 5) with their lengths, in bounded pages; continue
    /// with the cursor of [`DocPage::end`]. The reader adds the lengths up to
    /// compute progress. The format's own progress (op 17) has no wrapper in
    /// this SDK. `input` is 256 bytes; `out` 4,096.
    ///
    /// # Errors
    /// Invalid for a malformed page; State for a cursor whose list changed (start
    /// again); Busy while the document is preparing; otherwise the host's refusals.
    pub fn sections<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        cursor: DocCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DocPage<'a, SectionEntry>, Error> {
        let ask = wire::ServiceDocPage {
            document: document.0,
            scope: "",
            cursor: cursor.offset,
            generation: cursor.generation,
        };
        let n = self.call(host, schema::OP_SECTIONS, &[&ask], input, out)?;
        DocPage::decode(
            out.get(..n).ok_or(Error::Invalid)?,
            cursor,
            SectionEntry::decode,
        )
    }

    /// List the table of contents (op 7) in reading order, in bounded pages;
    /// continue with the cursor of [`DocPage::end`]. `input` is 256 bytes;
    /// `out` 4,096.
    ///
    /// # Errors
    /// Invalid for a malformed page (a level over 15 too); State for a cursor
    /// whose list changed (start again); Busy while the document is preparing;
    /// otherwise the host's refusals.
    pub fn toc<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        cursor: DocCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DocPage<'a, TocEntry<'a>>, Error> {
        let ask = wire::ServiceDocPage {
            document: document.0,
            scope: "",
            cursor: cursor.offset,
            generation: cursor.generation,
        };
        let n = self.call(host, schema::OP_TOC, &[&ask], input, out)?;
        DocPage::decode(
            out.get(..n).ok_or(Error::Invalid)?,
            cursor,
            TocEntry::decode,
        )
    }

    /// The exact position of a table-of-contents entry (op 8). `input` is 256
    /// bytes; `out` 4,096.
    ///
    /// # Errors
    /// NotFound for an entry the list does not have; Invalid for a malformed
    /// reply; Busy while preparing; otherwise the host's refusals.
    pub fn toc_target(
        &self,
        host: &Host<'_>,
        document: DocId,
        index: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<DocPosition, Error> {
        let ask = wire::ServiceDocIndex {
            document: document.0,
            index,
        };
        let n = self.call(host, schema::OP_TOC_TARGET, &[&ask], input, out)?;
        position_of(document, out.get(..n).ok_or(Error::Invalid)?)
    }

    /// List the links of the span's section that overlap its source range
    /// (op 12), in bounded pages; continue with the cursor of [`DocPage::end`].
    /// `input` is 256 bytes; `out` 4,096.
    ///
    /// # Errors
    /// Invalid for a span that ends before it starts (nothing is sent) or a malformed page; NotFound
    /// for a section past the end; State for a cursor whose list changed (start
    /// again); Busy while preparing; otherwise the host's refusals.
    pub fn links<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        span: DocSpan,
        cursor: DocCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DocPage<'a, Link<'a>>, Error> {
        if span.to < span.from {
            return Err(Error::Invalid);
        }
        let ask = wire::ServiceDocSpan {
            document: document.0,
            section: span.section,
            from: span.from,
            to: span.to,
            cursor: cursor.offset,
            generation: cursor.generation,
        };
        let n = self.call(host, schema::OP_LINKS, &[&ask], input, out)?;
        DocPage::decode(out.get(..n).ok_or(Error::Invalid)?, cursor, Link::decode)
    }

    /// The position an internal link points at (op 13). `input` is 2,048 bytes;
    /// `out` 4,096.
    ///
    /// # Errors
    /// Invalid for an `href` over 1,024 bytes (nothing is sent) or a malformed
    /// reply; NotFound for an external or dangling one; Busy while preparing;
    /// otherwise the host's refusals.
    pub fn link_target(
        &self,
        host: &Host<'_>,
        document: DocId,
        href: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<DocPosition, Error> {
        if href.len() > wire::SERVICE_DOC_HREF_HREF_MAX_BYTES {
            return Err(Error::Invalid);
        }
        let ask = wire::ServiceDocHref {
            document: document.0,
            href,
        };
        let n = self.call(host, schema::OP_LINK_TARGET, &[&ask], input, out)?;
        position_of(document, out.get(..n).ok_or(Error::Invalid)?)
    }
}

/// The one `doc_at` an op answers, as a position of `document`.
fn position_of(document: DocId, bytes: &[u8]) -> Result<DocPosition, Error> {
    let row = one(bytes)?
        .decode::<wire::ServiceDocAt>()
        .map_err(|_| Error::Invalid)?;
    if row.document != document.0 {
        return Err(Error::Invalid);
    }
    Ok(DocPosition::new(row.section, row.offset))
}
