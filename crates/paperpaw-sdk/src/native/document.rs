//! The document service `core.document` (`docs/abi/document.md` 2): open a book
//! through its format, read its preparation state, convert a position to a KOReader
//! XPointer and back, read its metadata, and close it. This is the opener's side; reading
//! blocks, tables of contents and the rest of the service's operations are not here yet.
use super::wire::services_wire::core_document_v1 as schema;
use super::{
    Error, Host,
    declare::records_wire::{self as wire, RawRecord, WireEncode, WireRecord},
    services::Service,
};

/// Most options one open forwards to its format (`document.md` 2.1).
pub const MAX_OPTIONS: usize = 12;
/// Longest side of a picture an open may ask for.
const IMAGE_SIDE_MAX: u32 = 1600;

/// A document the host opened for this plugin: an opaque id, never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocId(u32);
impl DocId {
    fn record(self) -> wire::ServiceDocRef {
        wire::ServiceDocRef { document: self.0 }
    }

    /// The id an event names; `None` for 0, which is never an id.
    pub(crate) const fn named(document: u32) -> Option<Self> {
        if document == 0 {
            None
        } else {
            Some(Self(document))
        }
    }
}

/// Why the host ended a document this plugin opened (`event.document_closed`,
/// `document.md` 2.1). Reasons this SDK does not know are [`Self::FormatStopped`],
/// as the contract requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocClosedReason {
    /// The format plugin stopped or was uninstalled (`reason` 1).
    FormatStopped,
    /// The card went (`reason` 2).
    CardRemoved,
    /// The book was moved or removed; for a handed file, its maker stopped
    /// (`reason` 3).
    BookGone,
    /// The place went to a book to show (`reason` 4); an index build tries again
    /// later.
    Needed,
}
impl DocClosedReason {
    pub(crate) const fn decode(reason: u32) -> Self {
        match reason {
            2 => Self::CardRemoved,
            3 => Self::BookGone,
            4 => Self::Needed,
            _ => Self::FormatStopped,
        }
    }
}

/// Where a path is read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Root {
    /// The plugin's own data folder.
    Data = 0,
    /// The card's library (root 1: the path of a book).
    Library = 1,
    /// The one file handed over to a reader instance, read only (`document.md` 2.1):
    /// a plugin's own file under its data or install folder. Only for the book,
    /// never for the CSS file.
    Handed = 5,
}

/// One book to open: its path, and how it is read.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct OpenBook<'a> {
    root: Root,
    path: &'a str,
    format: &'a str,
    flags: u32,
    image_side: u32,
    cover_side: u32,
    css: Option<(Root, &'a str)>,
    options: &'a [(&'a str, &'a str)],
}
impl<'a> OpenBook<'a> {
    /// A book on the card's library (root 1) at `path`, in the format the host
    /// chooses, with the host's picture sizes and publisher styles off.
    #[must_use]
    pub const fn new(path: &'a str) -> Self {
        Self {
            root: Root::Library,
            path,
            format: "",
            flags: 0,
            image_side: 0,
            cover_side: 0,
            css: None,
            options: &[],
        }
    }
    /// Read the path from another root.
    #[must_use]
    pub const fn root(mut self, root: Root) -> Self {
        self.root = root;
        self
    }
    /// Open with this format id (`<plugin>.<name>`) instead of the host's choice.
    #[must_use]
    pub const fn format(mut self, format: &'a str) -> Self {
        self.format = format;
        self
    }
    /// Apply the book's publisher styles.
    #[must_use]
    pub const fn publisher_styles(mut self) -> Self {
        self.flags |= 1;
        self
    }
    /// Ask for colour pictures.
    #[must_use]
    pub const fn color(mut self) -> Self {
        self.flags |= 1 << 1;
        self
    }
    /// An index build, not for showing: it may lose its place to a book to show.
    #[must_use]
    pub const fn background(mut self) -> Self {
        self.flags |= 1 << 2;
        self
    }
    /// The longest side of a page's picture, 1 to 1,600 (0 leaves it to the host).
    #[must_use]
    pub const fn image_side(mut self, side: u32) -> Self {
        self.image_side = side;
        self
    }
    /// The longest side of the cover (0 leaves it to the host). The SDK does not
    /// check it; the host bounds it.
    #[must_use]
    pub const fn cover_side(mut self, side: u32) -> Self {
        self.cover_side = side;
        self
    }
    /// A user CSS file on root 0 or 1.
    #[must_use]
    pub const fn css(mut self, root: Root, path: &'a str) -> Self {
        self.css = Some((root, path));
        self
    }
    /// The book's own setting values for its format: full setting ids and values.
    /// The host forwards those of the chosen format's plugin whole.
    #[must_use]
    pub const fn options(mut self, options: &'a [(&'a str, &'a str)]) -> Self {
        self.options = options;
        self
    }
}

/// What an open or a state query says of a document, borrowed from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocState<'a>(wire::ServiceDocState<'a>);

/// Where a document is in its preparation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Preparation {
    /// Ready to read.
    Ready,
    /// The format is still converting the book; ask again after the host's
    /// `document_changed` hint.
    Preparing,
    /// Failed; [`DocState::status`] says why. The id stays until it is closed.
    Failed,
}
impl<'a> DocState<'a> {
    /// The document's id.
    #[must_use]
    pub const fn id(&self) -> DocId {
        DocId(self.0.document)
    }
    /// Ready, preparing or failed. A state this SDK does not know reads as failed.
    #[must_use]
    pub const fn preparation(&self) -> Preparation {
        match self.0.state {
            0 => Preparation::Ready,
            1 => Preparation::Preparing,
            _ => Preparation::Failed,
        }
    }
    /// Progress in millionths while preparing.
    #[must_use]
    pub const fn progress_ppm(&self) -> u32 {
        self.0.progress_ppm
    }
    /// The status (an `ERR_*` number) of a failed document, else 0.
    #[must_use]
    pub const fn status(&self) -> i32 {
        self.0.status
    }
    /// The conversion the cache was made for, which positions and annotations are
    /// stored with.
    #[must_use]
    pub const fn conversion(&self) -> u64 {
        self.0.conversion
    }
    /// The id of the format that reads the book.
    #[must_use]
    pub const fn format(&self) -> &'a str {
        self.0.format
    }
    /// Each section is a whole-page picture.
    #[must_use]
    pub const fn whole_pages(&self) -> bool {
        self.0.flags & 1 != 0
    }
    /// The book has a table of contents.
    #[must_use]
    pub const fn has_toc(&self) -> bool {
        self.0.flags & (1 << 1) != 0
    }
    /// The book can convert positions to XPointers and back.
    #[must_use]
    pub const fn has_xpointers(&self) -> bool {
        self.0.flags & (1 << 2) != 0
    }
    /// The book embeds fonts.
    #[must_use]
    pub const fn has_fonts(&self) -> bool {
        self.0.flags & (1 << 3) != 0
    }
    /// The book has printed page labels.
    #[must_use]
    pub const fn has_page_labels(&self) -> bool {
        self.0.flags & (1 << 4) != 0
    }
    /// The pages run right to left.
    #[must_use]
    pub const fn right_to_left(&self) -> bool {
        self.0.flags & (1 << 5) != 0
    }
    /// The cache was made by this open.
    #[must_use]
    pub const fn cache_made_now(&self) -> bool {
        self.0.flags & (1 << 6) != 0
    }
}

/// A place in a document: a section and an offset in its source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocPosition {
    /// The section.
    pub section: u32,
    /// The offset in the section's source units.
    pub offset: u32,
}
impl DocPosition {
    /// The place at `offset` in `section`.
    #[must_use]
    pub const fn new(section: u32, offset: u32) -> Self {
        Self { section, offset }
    }
}

/// The document service handle. Release it explicitly within a callback.
#[derive(Debug)]
#[must_use = "release the document service handle when finished"]
pub struct Documents {
    service: Service,
}
impl Documents {
    /// Open the metadata-declared `core.document` service.
    ///
    /// # Errors
    /// Returns the host's admission, availability or allocation refusal.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }
    /// Release this service handle using the current callback's host.
    ///
    /// # Errors
    /// Returns the host's stale-handle or ownership refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }
    fn call(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[&dyn WireEncode],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let len = wire::records_encode_counted_of(
            records.iter().copied(),
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        self.service.call(host, op, &input[..len], out)
    }

    /// Open a book through its format (op 1). The answer is the document's state:
    /// ready, or preparing (read it again with [`state`](Self::state)), or failed
    /// (the id stays, with the status, until [`close`](Self::close)).
    ///
    /// `input` and `out` of 4,096 bytes always suffice.
    ///
    /// # Errors
    /// Invalid for an empty path, a path or format over its cap, a side above
    /// 1,600, or the handed root for the CSS file (nothing is sent); NoSpace for
    /// more than 12 options or one over its caps (never a silent drop); the host's
    /// refusals: `Denied`, `NotFound`, `Full` (the opener's or the device's open
    /// documents), `Busy`, `Unsupported` on a host that has not wired it.
    pub fn open_book<'a>(
        &self,
        host: &Host<'_>,
        book: &OpenBook<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DocState<'a>, Error> {
        let (css_root, css_path) = match book.css {
            Some((Root::Handed, _)) => return Err(Error::Invalid),
            Some((root, path)) => (root as u32, path),
            None => (0, ""),
        };
        if book.path.is_empty()
            || book.path.len() > wire::SERVICE_DOC_OPEN_PATH_MAX_BYTES
            || book.format.len() > wire::SERVICE_DOC_OPEN_FORMAT_MAX_BYTES
            || css_path.len() > wire::SERVICE_DOC_OPEN_CSS_PATH_MAX_BYTES
            || book.image_side > IMAGE_SIDE_MAX
        {
            return Err(Error::Invalid);
        }
        if book.options.len() > MAX_OPTIONS
            || book.options.iter().any(|(key, value)| {
                key.len() > wire::SERVICE_DOC_OPTION_KEY_MAX_BYTES
                    || value.len() > wire::SERVICE_DOC_OPTION_VALUE_MAX_BYTES
            })
        {
            return Err(Error::NoSpace);
        }
        let open = wire::ServiceDocOpen {
            root: book.root as u32,
            path: book.path,
            format: book.format,
            flags: book.flags,
            image_side: book.image_side,
            cover_side: book.cover_side,
            css_root,
            css_path,
        };
        let mut options = [wire::ServiceDocOption { key: "", value: "" }; MAX_OPTIONS];
        for (slot, (key, value)) in options.iter_mut().zip(book.options) {
            *slot = wire::ServiceDocOption { key, value };
        }
        let records: [&dyn WireEncode; MAX_OPTIONS + 1] =
            core::array::from_fn(|i| match i.checked_sub(1) {
                None => &open as &dyn WireEncode,
                Some(option) => &options[option],
            });
        let n = self.call(
            host,
            schema::OP_OPEN,
            &records[..=book.options.len()],
            input,
            out,
        )?;
        state_of(out.get(..n).ok_or(Error::Invalid)?)
    }

    /// Read a document's state (op 2), to follow its preparation.
    ///
    /// # Errors
    /// Cancelled for a document the host closed (close it to forget it); the host's
    /// other refusals; Invalid for a malformed reply.
    pub fn state<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<DocState<'a>, Error> {
        let n = self.call(host, schema::OP_STATE, &[&document.record()], input, out)?;
        let state = state_of(out.get(..n).ok_or(Error::Invalid)?)?;
        if state.id() != document {
            return Err(Error::Invalid);
        }
        Ok(state)
    }

    /// Close a document (op 18): the format frees its state and its images end.
    /// Close a document the host has already ended (`event.document_closed`) too:
    /// that is what forgets its id.
    ///
    /// # Errors
    /// Invalid for a reply that is not empty; the host's refusals.
    pub fn close(
        &self,
        host: &Host<'_>,
        document: DocId,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if self.call(host, schema::OP_CLOSE, &[&document.record()], input, out)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// The KOReader XPointer of a position (op 10), borrowed from `out`.
    ///
    /// # Errors
    /// NotFound where the format has no XPointers (see
    /// [`DocState::has_xpointers`]) or the section is past the end; Busy while the
    /// document is preparing; Invalid for a malformed reply; otherwise the host's
    /// refusals.
    pub fn xpointer<'a>(
        &self,
        host: &Host<'_>,
        document: DocId,
        at: DocPosition,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<&'a str, Error> {
        let at = wire::ServiceDocAt {
            document: document.0,
            section: at.section,
            offset: at.offset,
            insert: 0,
            cursor: 0,
            generation: 0,
        };
        let n = self.call(host, schema::OP_XPOINTER, &[&at], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceDocXpointer<'a>>()
            .map_err(|_| Error::Invalid)?;
        if row.document != document.0 {
            return Err(Error::Invalid);
        }
        Ok(row.text)
    }

    /// The position of a KOReader XPointer (op 11).
    ///
    /// # Errors
    /// Invalid for an XPointer over 1,024 bytes (nothing is sent) or a malformed
    /// reply; NotFound for one the book does not have; Busy while preparing;
    /// otherwise the host's refusals.
    pub fn locate(
        &self,
        host: &Host<'_>,
        document: DocId,
        xpointer: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<DocPosition, Error> {
        if xpointer.len() > wire::SERVICE_DOC_XPOINTER_TEXT_MAX_BYTES {
            return Err(Error::Invalid);
        }
        let ask = wire::ServiceDocXpointer {
            document: document.0,
            text: xpointer,
        };
        let n = self.call(host, schema::OP_LOCATE, &[&ask], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceDocAt>()
            .map_err(|_| Error::Invalid)?;
        if row.document != document.0 {
            return Err(Error::Invalid);
        }
        Ok(DocPosition {
            section: row.section,
            offset: row.offset,
        })
    }
}

#[path = "document_readers.rs"]
mod readers;
pub use readers::{Reader, ReaderCursor, ReaderPage, ReaderPageEnd};
#[path = "document_nav.rs"]
mod nav;
pub use nav::{DocCursor, DocPage, DocPageEnd, DocSpan, Link, LinkKind, SectionEntry, TocEntry};
#[path = "document_blocks.rs"]
mod blocks;
pub use blocks::{
    BlockCell, BlockColumns, BlockCursor, BlockFragment, BlockHead, BlockImage, BlockMap,
    BlockRecord, BlockRuby, BlockRun,
};
#[path = "document_meta.rs"]
mod meta;
pub use meta::{DocTextPart, DocTextQuery, Metadata};
#[path = "document_report.rs"]
mod report;
pub use report::Report;
#[path = "document_formats.rs"]
mod formats;
pub use formats::{Format, FormatCursor, FormatPage, FormatPageEnd};

fn one(bytes: &[u8]) -> Result<RawRecord<'_>, Error> {
    RawRecord::one(bytes).map_err(|_| Error::Invalid)
}
fn state_of(bytes: &[u8]) -> Result<DocState<'_>, Error> {
    let row = one(bytes)?
        .decode::<wire::ServiceDocState<'_>>()
        .map_err(|_| Error::Invalid)?;
    // An id is never 0.
    if row.document == 0 {
        return Err(Error::Invalid);
    }
    Ok(DocState(row))
}

#[cfg(test)]
#[path = "document_nav_tests.rs"]
mod nav_tests;
#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
