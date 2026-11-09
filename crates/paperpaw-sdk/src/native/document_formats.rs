//! The formats of the device (`docs/abi/document.md` 1 and 2, ops 19 and 20):
//! which format reads a file, for "Open with" and a file manager's Open.
use super::*;

/// The flag of `doc_path`: sniff when no extension matches.
const SNIFF: u32 = 1;

/// One format, borrowed from the caller's reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format<'a>(wire::ServiceDocFormat<'a>);

impl<'a> Format<'a> {
    fn check(row: wire::ServiceDocFormat<'a>) -> Result<Self, Error> {
        // An id, and at most 16 extensions, none empty (`format.spec`).
        if row.id.is_empty()
            || !row.extensions.is_empty()
                && (row
                    .extensions
                    .split(wire::SERVICE_DOC_FORMAT_EXTENSIONS_SEPARATOR)
                    .any(str::is_empty)
                    || row
                        .extensions
                        .split(wire::SERVICE_DOC_FORMAT_EXTENSIONS_SEPARATOR)
                        .count()
                        > wire::SERVICE_DOC_FORMAT_EXTENSIONS_MAX_ITEMS)
        {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }

    /// The format's id, `<plugin>.<name>` (at most 128 bytes): the scope of
    /// [`Documents::readers`] and the `format` of an open.
    #[must_use]
    pub const fn id(&self) -> &'a str {
        self.0.id
    }

    /// The format's title, translated by the host (at most 128 bytes).
    #[must_use]
    pub const fn title(&self) -> &'a str {
        self.0.title
    }

    /// The extensions the format claims, lowercase without the dot (`epub`,
    /// `fb2.zip`), at most 16.
    pub fn extensions(&self) -> impl Iterator<Item = &'a str> + use<'a> {
        self.0
            .extensions
            .split(wire::SERVICE_DOC_FORMAT_EXTENSIONS_SEPARATOR)
            .filter(|extension| !extension.is_empty())
    }

    /// The format's priority among formats claiming one extension (higher wins).
    #[must_use]
    pub const fn priority(&self) -> i32 {
        self.0.priority
    }
}

/// Where a page of formats continues; start with [`FormatCursor::start`] and
/// continue only with the cursor a page returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatCursor {
    offset: u32,
    generation: u32,
}

impl FormatCursor {
    /// The first page.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
}

/// How a page of formats ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatPageEnd {
    total: u32,
    next: Option<FormatCursor>,
}

impl FormatPageEnd {
    /// The formats of the whole list.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }

    /// Where the next page starts, or `None` after the last page.
    #[must_use]
    pub const fn next(self) -> Option<FormatCursor> {
        self.next
    }
}

/// A page of formats, validated in full before any row is exposed.
#[derive(Debug)]
pub struct FormatPage<'a> {
    bytes: &'a [u8],
    count: usize,
    end: FormatPageEnd,
}

impl<'a> FormatPage<'a> {
    fn decode(bytes: &'a [u8], cursor: FormatCursor) -> Result<Self, Error> {
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
            let row = record
                .decode::<wire::ServiceDocFormat<'a>>()
                .map_err(|_| Error::Invalid)?;
            Format::check(row)?;
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
            end: FormatPageEnd {
                total: end.total,
                next: (!last).then_some(FormatCursor {
                    offset: end.next,
                    generation: end.generation,
                }),
            },
        })
    }

    /// The formats of this page and where it ends.
    #[must_use]
    pub const fn end(&self) -> FormatPageEnd {
        self.end
    }

    /// Number of formats in this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether this page has no formats.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterate the borrowed formats without allocation.
    pub fn rows(&self) -> impl Iterator<Item = Format<'a>> + use<'a> {
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
                .decode::<wire::ServiceDocFormat<'a>>()
                .ok()
                .map(Format)
        })
    }
}

impl Documents {
    /// List every format (op 19), in bounded pages. Continue with the cursor of
    /// the page's [`FormatPage::end`]. No permission is needed. `input` is 256
    /// bytes; `out` 4,096.
    ///
    /// # Errors
    /// Invalid for a malformed page (also a format with no id or an empty
    /// extension); State for a cursor whose list changed (start again from
    /// [`FormatCursor::start`]); otherwise the host's refusals.
    pub fn formats<'a>(
        &self,
        host: &Host<'_>,
        cursor: FormatCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<FormatPage<'a>, Error> {
        let ask = wire::ServiceDocPage {
            document: 0,
            scope: "",
            cursor: cursor.offset,
            generation: cursor.generation,
        };
        let n = self.call(host, schema::OP_FORMATS, &[&ask], input, out)?;
        FormatPage::decode(out.get(..n).ok_or(Error::Invalid)?, cursor)
    }

    /// Which format reads the file at `path` of `root` (op 20): by its extension
    /// (the user's choice, else the higher priority), and when no extension matches and
    /// `sniff` is set, by the first 512 bytes of the file offered to the formats
    /// that sniff. `root` is [`Root::Data`] or [`Root::Library`] (the user's
    /// files, which needs `uses.storage`). `input` is 1,024 bytes; `out` 1,536.
    ///
    /// # Errors
    /// `NotFound` when no format reads the file; Invalid for an empty path or one
    /// over 512 bytes, or [`Root::Handed`] (nothing is sent), or a malformed
    /// reply; `Denied` for the user's files without `uses.storage`; otherwise the
    /// host's refusals.
    pub fn format_for<'a>(
        &self,
        host: &Host<'_>,
        root: Root,
        path: &str,
        sniff: bool,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Format<'a>, Error> {
        if path.is_empty()
            || path.len() > wire::SERVICE_DOC_PATH_PATH_MAX_BYTES
            || matches!(root, Root::Handed)
        {
            return Err(Error::Invalid);
        }
        let ask = wire::ServiceDocPath {
            root: root as u32,
            path,
            flags: u32::from(sniff) * SNIFF,
        };
        let n = self.call(host, schema::OP_FORMAT_FOR, &[&ask], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceDocFormat<'a>>()
            .map_err(|_| Error::Invalid)?;
        Format::check(row)
    }
}
