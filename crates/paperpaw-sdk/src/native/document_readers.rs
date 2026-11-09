//! The reading apps of the device (`docs/abi/document.md` 2, op 21): who can open
//! which formats, for "Open with" and the default reading app.
use super::*;

/// Where a page of readers continues; start with [`ReaderCursor::start`] and
/// continue only with the cursor a page returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReaderCursor {
    offset: u32,
    generation: u32,
}

impl ReaderCursor {
    /// The first page.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
}

/// One reading app, borrowed from the caller's reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reader<'a>(wire::ServiceDocReader<'a>);

impl<'a> Reader<'a> {
    /// The reader's id, `<plugin>.<name>` (at most 128 bytes).
    #[must_use]
    pub const fn id(&self) -> &'a str {
        self.0.id
    }

    /// The screen that opens a book as this reader: the target of `command.open`
    /// with the slot `core.reader` (at most 128 bytes).
    #[must_use]
    pub const fn screen(&self) -> &'a str {
        self.0.screen
    }

    /// The reader's title, translated by the host (at most 128 bytes).
    #[must_use]
    pub const fn title(&self) -> &'a str {
        self.0.title
    }

    /// The format ids this reader shows, at most 8; empty for every format (see
    /// [`reads_every_format`](Self::reads_every_format)).
    pub fn formats(&self) -> impl Iterator<Item = &'a str> + use<'a> {
        self.0
            .formats
            .split(wire::SERVICE_DOC_READER_FORMATS_SEPARATOR)
            .filter(|id| !id.is_empty())
    }

    /// Whether the reader declared no formats, which means every format.
    #[must_use]
    pub const fn reads_every_format(&self) -> bool {
        self.0.formats.is_empty()
    }
}

/// How a page of readers ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderPageEnd {
    total: u32,
    next: Option<ReaderCursor>,
}

impl ReaderPageEnd {
    /// The readers of the whole list.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }

    /// Where the next page starts, or `None` after the last page.
    #[must_use]
    pub const fn next(self) -> Option<ReaderCursor> {
        self.next
    }
}

/// A page of readers, validated in full before any row is exposed.
#[derive(Debug)]
pub struct ReaderPage<'a> {
    bytes: &'a [u8],
    count: usize,
    end: ReaderPageEnd,
}

impl<'a> ReaderPage<'a> {
    fn decode(bytes: &'a [u8], cursor: ReaderCursor) -> Result<Self, Error> {
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
            record
                .decode::<wire::ServiceDocReader<'a>>()
                .map_err(|_| Error::Invalid)?;
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
            end: ReaderPageEnd {
                total: end.total,
                next: (!last).then_some(ReaderCursor {
                    offset: end.next,
                    generation: end.generation,
                }),
            },
        })
    }

    /// The readers of this page and where it ends.
    #[must_use]
    pub const fn end(&self) -> ReaderPageEnd {
        self.end
    }

    /// Number of readers in this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether this page has no readers.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterate the borrowed readers without allocation.
    pub fn rows(&self) -> impl Iterator<Item = Reader<'a>> + use<'a> {
        let mut bytes = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let (record, n) = RawRecord::prefix(bytes).ok()?;
            bytes = bytes.get(n..)?;
            record
                .decode::<wire::ServiceDocReader<'a>>()
                .ok()
                .map(Reader)
        })
    }
}

impl Documents {
    /// List the reading apps (op 21), in bounded pages; `scope` keeps those that
    /// show the format with this id, or all of them when empty. Continue with the
    /// cursor of the page's [`ReaderPage::end`]. No permission is needed. `input`
    /// is 256 bytes; `out` 4,096.
    ///
    /// # Errors
    /// Invalid for a scope over 128 bytes (nothing is sent) or a malformed page;
    /// State for a cursor whose list changed (start again from
    /// [`ReaderCursor::start`]); otherwise the host's refusals.
    pub fn readers<'a>(
        &self,
        host: &Host<'_>,
        scope: &str,
        cursor: ReaderCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<ReaderPage<'a>, Error> {
        if scope.len() > wire::SERVICE_DOC_PAGE_SCOPE_MAX_BYTES {
            return Err(Error::Invalid);
        }
        let ask = wire::ServiceDocPage {
            document: 0,
            scope,
            cursor: cursor.offset,
            generation: cursor.generation,
        };
        let n = self.call(host, schema::OP_READERS, &[&ask], input, out)?;
        ReaderPage::decode(out.get(..n).ok_or(Error::Invalid)?, cursor)
    }
}
