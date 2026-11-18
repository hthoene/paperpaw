use super::{Error, schema};

/// The operations of `core.format` major 1, distinct from service operation ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatOp {
    /// Inspect the first bytes of a file.
    Sniff,
    /// Open a document using the scoped roots.
    Open,
    /// Continue bounded incremental preparation.
    Prepare,
    /// Release the document state.
    Close,
    /// Read document metadata.
    Meta,
    /// Read metadata text from an offset.
    MetaText,
    /// Read a page of sections.
    Sections,
    /// Read one bounded block fragment.
    Block,
    /// Read a page of table-of-contents entries.
    Toc,
    /// Resolve one table-of-contents entry.
    TocTarget,
    /// Read a printed page label.
    PageLabel,
    /// Encode a position as an XPointer.
    Xpointer,
    /// Resolve an XPointer.
    Locate,
    /// Read links overlapping a source range.
    Links,
    /// Resolve a link within the book.
    LinkTarget,
    /// Read a page of embedded font faces.
    Fonts,
    /// Prepare the cover image.
    Cover,
    /// Prepare a document image.
    Image,
    /// Read the progress at a source position.
    Progress,
}
impl FormatOp {
    fn decode(op: u32) -> Result<Self, Error> {
        Ok(match op {
            1 => Self::Sniff,
            2 => Self::Open,
            3 => Self::Prepare,
            4 => Self::Close,
            5 => Self::Meta,
            6 => Self::MetaText,
            7 => Self::Sections,
            8 => Self::Block,
            9 => Self::Toc,
            10 => Self::TocTarget,
            11 => Self::PageLabel,
            12 => Self::Xpointer,
            13 => Self::Locate,
            14 => Self::Links,
            15 => Self::LinkTarget,
            16 => Self::Fonts,
            17 => Self::Cover,
            18 => Self::Image,
            19 => Self::Progress,
            _ => return Err(Error::Unsupported),
        })
    }
}

/// A validated request borrowing the host's stage payload. `Debug` shows lengths
/// and counts, never the sniffed file bytes, the text or the option values.
#[derive(Clone)]
pub struct FormatCall<'a> {
    pub(super) row: schema::FmtCall<'a>,
    op: FormatOp,
    options: schema::RecordStream<'a>,
}
impl core::fmt::Debug for FormatCall<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FormatCall")
            .field("op", &self.op)
            .field("document", &self.row.document)
            .field("conversion", &self.row.conversion)
            .field("text_len", &self.row.text.len())
            .field("sniff_len", &self.row.arg.len())
            .field("options", &self.options.len())
            .finish_non_exhaustive()
    }
}
impl<'a> FormatCall<'a> {
    /// Validate framing and the operation before exposing any request fields.
    ///
    /// # Errors
    /// Unsupported for an unknown operation; Invalid for malformed input,
    /// a zero document outside sniff, or options outside open. Generated caps
    /// and field validation errors pass through unchanged.
    pub fn decode(payload: &'a [u8]) -> Result<Self, Error> {
        let mut rows = schema::input_decode(payload).map_err(Error::from_status)?;
        let Some(Ok(schema::Record::FmtCall(row))) = rows.next() else {
            return Err(Error::Invalid);
        };
        let op = FormatOp::decode(row.op)?;
        if op != FormatOp::Sniff && row.document == 0 {
            return Err(Error::Invalid);
        }
        if (op == FormatOp::Open && (row.flags & !7 != 0 || row.image_side > 1600))
            || (op == FormatOp::Image && row.image_side > 1600)
            || (op == FormatOp::MetaText && row.flags > 4)
        {
            return Err(Error::Invalid);
        }
        if op != FormatOp::Open && !rows.is_empty() {
            return Err(Error::Invalid);
        }
        if rows.len() > 12 {
            return Err(Error::NoSpace);
        }
        for option in rows.clone() {
            let schema::Record::FmtOption(option) = option.map_err(Error::from_status)? else {
                return Err(Error::Invalid);
            };
            super::replies::validate_id(option.key)?;
        }
        Ok(Self {
            row,
            op,
            options: rows,
        })
    }
    /// The provider operation; these ids differ from `core.document` ops.
    #[must_use]
    pub const fn operation(&self) -> FormatOp {
        self.op
    }
    /// Borrow the forwarded setting options without collecting them.
    #[must_use]
    pub fn options(&self) -> FormatOptions<'a> {
        FormatOptions(self.options.clone())
    }
    /// The host-assigned document id; sniff may use zero.
    #[must_use]
    pub const fn document(&self) -> u32 {
        self.row.document
    }
    /// The source section of a positional call.
    #[must_use]
    pub const fn section(&self) -> u32 {
        self.row.section
    }
    /// The source offset, or the start of a link range.
    #[must_use]
    pub const fn offset(&self) -> u32 {
        self.row.offset
    }
    /// The requested entry or image index, or the end of a link range.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.row.index
    }
    /// The continuation cursor, zero for the first page or fragment.
    #[must_use]
    pub const fn cursor(&self) -> u32 {
        self.row.cursor
    }
    /// The generation of a continuation, zero for a first request.
    #[must_use]
    pub const fn generation(&self) -> u32 {
        self.row.generation
    }
    /// Open flags, or the metadata text kind for `MetaText`.
    #[must_use]
    pub const fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Maximum image side in pixels, 1 to 1,600 on open and image: the host turns a
    /// requested 0 into its default before it calls (`document.md` 3), so a format
    /// never sees 0 there. It is 0 on the other operations.
    #[must_use]
    pub const fn image_side(&self) -> u32 {
        self.row.image_side
    }
    /// Maximum cover side in pixels. The host has already replaced a requested 0
    /// with its default and keeps it within the screen's long side (`document.md`
    /// 3); a format does not bound it again. It is 0 on the other operations.
    #[must_use]
    pub const fn cover_side(&self) -> u32 {
        self.row.cover_side
    }
    /// The cache conversion identity assigned by the host.
    #[must_use]
    pub const fn conversion(&self) -> u64 {
        self.row.conversion
    }
    /// The XPointer, href, or siblings-format file name, according to the operation.
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.row.text
    }
    /// The initial file bytes supplied for sniffing.
    #[must_use]
    pub const fn sniff_bytes(&self) -> &'a [u8] {
        self.row.arg
    }
    /// Whether open asks to apply publisher styles.
    #[must_use]
    pub const fn publisher_styles(&self) -> bool {
        self.row.flags & 1 != 0
    }
    /// Whether open asks for color pictures.
    #[must_use]
    pub const fn color(&self) -> bool {
        self.row.flags & 2 != 0
    }
    /// Whether open is a background index build.
    #[must_use]
    pub const fn background(&self) -> bool {
        self.row.flags & 4 != 0
    }
}

/// One borrowed option forwarded to this format owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatOption<'a>(schema::FmtOption<'a>);
impl<'a> FormatOption<'a> {
    /// Full setting id.
    #[must_use]
    pub const fn key(self) -> &'a str {
        self.0.key
    }
    /// The book's setting value.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.0.value
    }
}
/// A lending-free iterator over the already validated request options.
#[derive(Clone, Debug)]
pub struct FormatOptions<'a>(schema::RecordStream<'a>);
impl<'a> Iterator for FormatOptions<'a> {
    type Item = Result<FormatOption<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|row| match row.map_err(Error::from_status)? {
                schema::Record::FmtOption(option) => Ok(FormatOption(option)),
                _ => Err(Error::Invalid),
            })
    }
}
