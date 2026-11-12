//! The interface-7 `core.format` provider of Word documents
//! (`docs/abi/document.md` section 3): one `fmt.call` in, the op's records
//! out. The book is root 3 (path `""`, the archive), what is made of it root 6
//! (the cache's own files: `blocks.bin`, `index.bin`, `links.bin`, `toc.idx`,
//! `toc.bin`, `urls.*`, `marks.bin`, `pictures.txt`, `info.txt`, the pictures
//! `media-<n>` and `img-<n>.gray`, and `done`, written last); the host owns
//! the cache's stamp (`.complete`).
//!
//! The making of the cache is op 3 `prepare`: the whole conversion in the
//! first call (a streaming read of the archive that cannot be resumed between
//! calls: see `PORTING-7.md`), answering `state` 0 when it is done. A document
//! that is no Word document, or damaged, opens as a failed document.
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::min;

use paperpaw_sdk::document::{Block, BlockKind, Document, DocumentError, ImageRef, Locator};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::native::{
    Error, Host,
    stage_schemas::core_format_v1::{self as stage, Record},
    storage::{self, Root},
};
use paperpaw_sdk::{DirEntry, IMAGE_FIT, Storage, StorageError};

use crate::cache;
use crate::document::DocxDocument;
use crate::images;

mod op {
    pub const OPEN: u32 = 2;
    pub const PREPARE: u32 = 3;
    pub const CLOSE: u32 = 4;
    pub const META: u32 = 5;
    pub const META_TEXT: u32 = 6;
    pub const SECTIONS: u32 = 7;
    pub const BLOCK: u32 = 8;
    pub const TOC: u32 = 9;
    pub const TOC_TARGET: u32 = 10;
    pub const LINKS: u32 = 14;
    pub const LINK_TARGET: u32 = 15;
    pub const COVER: u32 = 17;
    pub const IMAGE: u32 = 18;
}

/// Open documents (the host allows 8 elsewhere, 2 on the X4).
pub const MAX_DOCUMENTS: usize = 8;
/// The format's id, as `crate::FORMAT` spells it.
const FORMAT_ID: &str = "format-docx.docx";
/// The code a refused allocation travels as through `StorageError`.
const NO_MEMORY: u16 = 0x6e6d;
/// The most a `fmt.text` carries, and a `fmt.text_part`.
const TEXT: usize = 1024;
const PART: usize = 3072;
/// Rows of a reply, one of them the closing `fmt.end`.
const ROWS: usize = 64;
const END_LAST: u32 = 1;
const END_MORE: u32 = 2;
/// The `generation` of a paged list (`document.md` section 3: the format's own for the
/// document, never 0): the lists of a document do not change, so one value serves.
const LIST_GENERATION: u32 = 1;
const STATE_READY: u32 = 0;
const STATE_MORE: u32 = 1;
const STATE_FAILED: u32 = 2;
const FLAG_TOC: u32 = 1 << 1;
const CACHE_MADE: u32 = 1 << 6;
/// Bit 1 of an open's `flags`: pictures in colour.
const OPEN_COLOR: u32 = 1 << 1;
/// Bit 3 of an open's `flags`: details (the library's book details or cover;
/// not for showing). Bit 2, background (an index build), converts as usual.
const OPEN_DETAILS: u32 = 1 << 3;
/// The marker that the cache is whole, written when the last record is done.
const DONE: &str = cache::DONE;

/// The book (path `""`) and the cache (any other path) as one storage.
pub struct HostIo<'h, 'cb> {
    host: &'h Host<'cb>,
}

impl<'h, 'cb> HostIo<'h, 'cb> {
    /// The roots of the call `host` belongs to.
    #[must_use]
    pub const fn new(host: &'h Host<'cb>) -> Self {
        Self { host }
    }
}

impl core::fmt::Debug for HostIo<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostIo").finish_non_exhaustive()
    }
}

fn route(path: &str) -> (Root, &str) {
    if path.is_empty() {
        (Root::Book, "")
    } else {
        (Root::DocumentCache, path)
    }
}

fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        // A refused allocation is not a damaged book: it keeps its name.
        Error::NoMemory => StorageError::Other(NO_MEMORY),
        _ => StorageError::Io,
    }
}

impl Storage for HostIo<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let (root, name) = route(path);
        let mut done = 0;
        while done < buf.len() {
            let end = min(buf.len(), done + storage::MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n = storage::read_at(self.host, root, name, at, part).map_err(storage_error)?;
            done += n;
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (root, name) = route(path);
        storage::size(self.host, root, name).map_err(storage_error)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (root, name) = route(path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        if data.is_empty() {
            return storage::write(self.host, root, name, data).map_err(storage_error);
        }
        // The first piece makes the file new, the rest follow it.
        let mut pieces = data.chunks(storage::MAX_DATA);
        if let Some(first) = pieces.next() {
            storage::write(self.host, root, name, first).map_err(storage_error)?;
        }
        for piece in pieces {
            storage::append(self.host, root, name, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (root, name) = route(path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        for piece in data.chunks(storage::MAX_DATA) {
            storage::append(self.host, root, name, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let (root, name) = route(path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        storage::remove(self.host, root, name).map_err(storage_error)
    }
}

#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "at most 8 slots (2 on the X4); a Box would allocate where a refusal aborts (no Box::try_new)"
)]
enum Body {
    /// The cache is not made yet (the first `prepare` makes it): the page's
    /// picture fit, colour.
    Preparing(Fit, bool),
    Ready(DocxDocument),
    /// A details open (the library's details) of a book whose cache is
    /// not made: only what the properties say (`meta`, `meta_text`); every
    /// other ask is `State`, and nothing is converted or written.
    Details(DocxDocument),
    /// `Err(status)`: the open failed, the id stays until it is closed.
    Failed(i32),
}

#[derive(Debug)]
struct Slot {
    id: u32,
    conversion: u64,
    body: Body,
}

/// The Word documents format's documents.
#[derive(Debug, Default)]
pub struct DocxProvider {
    slots: Vec<Slot>,
}

/// The longest prefix of `text` of at most `max` bytes, cut at a character.
fn prefix(text: &str, max: usize) -> &str {
    let mut end = min(max, text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

fn document_error(error: &DocumentError) -> Error {
    match error {
        DocumentError::Storage(StorageError::NotFound) => Error::NotFound,
        DocumentError::Storage(StorageError::Full) => Error::Full,
        DocumentError::Storage(StorageError::Unsupported) | DocumentError::UnsupportedFormat => {
            Error::Unsupported
        }
        DocumentError::OutOfMemory | DocumentError::Storage(StorageError::Other(NO_MEMORY)) => {
            Error::NoMemory
        }
        DocumentError::Malformed(_) => Error::Invalid,
        DocumentError::NotReady => Error::Busy,
        _ => Error::Io,
    }
}

fn one(record: Record<'_>, out: &mut [u8]) -> Result<usize, Error> {
    stage::reply_encode(&[record], out).map_err(|_| Error::NoSpace)
}

fn state_record(
    id: u32,
    conversion: u64,
    state: u32,
    ppm: u32,
    status: i32,
    flags: u32,
) -> Record<'static> {
    Record::FmtState(stage::FmtState::new(
        id, state, ppm, status, flags, conversion, FORMAT_ID,
    ))
}

/// A page of `rows` then the closing `fmt.end`, the most rows that fit `out`.
/// `from` is the first row's index in the whole list of `total`.
fn page(rows: &[Record<'_>], from: usize, total: usize, out: &mut [u8]) -> Result<usize, Error> {
    let mut frame: Vec<Record<'_>> = Vec::new();
    frame.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
    let mut take = min(rows.len(), ROWS - 1);
    loop {
        let next = from + take;
        let more = next < total;
        frame.clear();
        frame.extend(rows.iter().take(take).cloned());
        frame.push(Record::FmtEnd(stage::FmtEnd::new(
            if more { 0 } else { END_LAST },
            u32::try_from(total).map_err(|_| Error::Invalid)?,
            LIST_GENERATION,
            u32::try_from(next).map_err(|_| Error::Invalid)?,
            None,
        )));
        match stage::reply_encode(&frame, out) {
            Ok(written) => return Ok(written),
            Err(_) if take <= 1 || rows.is_empty() => return Err(Error::NoSpace),
            Err(_) => take -= 1,
        }
    }
}

/// The side a call names (0: the opener's own default), as a fit.
fn fit_of(side: u32, default: Fit) -> Fit {
    if side == 0 { default } else { Fit::side(side) }
}

/// What a link of the document leads to: a web address of the document, or
/// a place in it (`#<number>`, as the host asks for it in op 15).
enum Href {
    Place(([u8; 11], usize)),
    Address(String),
}

impl Href {
    fn text(&self) -> Option<&str> {
        match self {
            Href::Place((bytes, len)) => bytes
                .get(..*len)
                .and_then(|bytes| core::str::from_utf8(bytes).ok()),
            Href::Address(url) => Some(url),
        }
    }
}

/// The addresses of one page of links, at most, in bytes (a reply holds
/// 3,840).
const ADDRESS_BYTES: usize = 2_500;

/// `#<value>`, on the stack.
fn place(value: u32) -> ([u8; 11], usize) {
    let mut digits = [0u8; 10];
    let mut n = value;
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let mut out = [b'#'; 11];
    let count = digits.len() - at;
    out[1..=count].copy_from_slice(&digits[at..]);
    (out, count + 1)
}

impl DocxProvider {
    /// A provider with no document open.
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Open documents.
    #[must_use]
    pub fn documents(&self) -> usize {
        self.slots.len()
    }

    /// Answers one `core.format` call: `input` is its payload, the reply goes
    /// into `out` and its length is returned.
    ///
    /// # Errors
    /// The status the host passes on: `Invalid` for a payload that is not one
    /// `fmt.call` or names an open document twice, `NotFound` for a document or
    /// a place that is not there, `Busy` while it prepares, `NoMemory` where
    /// memory is short (nothing is lost: the call may be made again),
    /// `Unsupported` for the ops a Word document has no answer to, `NoSpace`
    /// where `out` is too small.
    pub fn call(&mut self, io: &dyn Storage, input: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let mut records = stage::input_decode(input).map_err(|_| Error::Invalid)?;
        let Some(Ok(Record::FmtCall(call))) = records.next() else {
            return Err(Error::Invalid);
        };
        match call.op {
            // Not a sniffer: no rows (the extension names the format).
            1 => stage::reply_encode(&[], out).map_err(|_| Error::NoSpace),
            op::OPEN => self.open(io, &call, out),
            op::PREPARE => self.prepare(io, &call, out),
            op::CLOSE => {
                if let Some(at) = self.slots.iter().position(|s| s.id == call.document) {
                    self.slots.swap_remove(at);
                }
                stage::reply_encode(&[], out).map_err(|_| Error::NoSpace)
            }
            op::BLOCK => {
                let conversion = self.slot(call.document)?.conversion;
                let document = self.ready(call.document)?;
                Self::block(document, io, conversion, &call, out)
            }
            _ => {
                let document = self.readable(call.document, call.op)?;
                Self::ask(document, io, &call, out)
            }
        }
    }

    fn slot(&self, id: u32) -> Result<&Slot, Error> {
        self.slots
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::NotFound)
    }

    fn ready(&self, id: u32) -> Result<&DocxDocument, Error> {
        match &self.slot(id)?.body {
            Body::Ready(document) => Ok(document),
            Body::Preparing(..) => Err(Error::Busy),
            Body::Failed(_) | Body::Details(_) => Err(Error::State),
        }
    }

    /// The document of `id` for `op`: a details open answers only the
    /// properties' ops and the cover (a Word document has none), the others
    /// are `State`.
    fn readable(&self, id: u32, op: u32) -> Result<&DocxDocument, Error> {
        match &self.slot(id)?.body {
            Body::Details(document) if matches!(op, op::META | op::META_TEXT | op::COVER) => {
                Ok(document)
            }
            _ => self.ready(id),
        }
    }

    fn open(
        &mut self,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        if self.slots.iter().any(|s| s.id == call.document) {
            return Err(Error::Invalid);
        }
        if self.slots.len() >= MAX_DOCUMENTS {
            return Err(Error::Full);
        }
        self.slots.try_reserve(1).map_err(|_| Error::NoMemory)?;
        let (body, made) = match start(io, call) {
            Ok(started) => started,
            // Short memory is for the host to try again, not a failed book.
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(error) => (Body::Failed(i32::from(error)), false),
        };
        let record = match &body {
            Body::Ready(document) => state_record(
                call.document,
                call.conversion,
                STATE_READY,
                0,
                0,
                ready_flags(document, io, made)?,
            ),
            // Ready at once, with nothing made: no show-only flags.
            Body::Details(_) => state_record(call.document, call.conversion, STATE_READY, 0, 0, 0),
            Body::Preparing(..) => {
                state_record(call.document, call.conversion, STATE_MORE, 0, 0, 0)
            }
            Body::Failed(status) => {
                state_record(call.document, call.conversion, STATE_FAILED, 0, *status, 0)
            }
        };
        let written = one(record, out)?;
        self.slots.push(Slot {
            id: call.document,
            conversion: call.conversion,
            body,
        });
        Ok(written)
    }

    fn prepare(
        &mut self,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let slot = self
            .slots
            .iter_mut()
            .find(|s| s.id == call.document)
            .ok_or(Error::NotFound)?;
        let (id, conversion) = (slot.id, slot.conversion);
        let (fit, color) = match &slot.body {
            Body::Ready(_) => return one(state_record(id, conversion, STATE_READY, 0, 0, 0), out),
            // A details open is ready from the open; there is nothing to prepare.
            Body::Details(_) => return Err(Error::State),
            Body::Failed(status) => {
                return one(
                    state_record(id, conversion, STATE_FAILED, 0, *status, 0),
                    out,
                );
            }
            Body::Preparing(fit, color) => (*fit, *color),
        };
        // What an earlier call left is not to be trusted: made from the start.
        clear(io);
        let made = DocxDocument::convert(io, "", "")
            .and_then(|()| io.write(DONE, b"1").map_err(DocumentError::from))
            .and_then(|()| DocxDocument::native(io, fit, color));
        let document = match made {
            Ok(document) => document,
            Err(error) => {
                clear(io);
                let status = document_error(&error);
                if status == Error::NoMemory {
                    // Nothing is lost: the next call makes it again.
                    return Err(Error::NoMemory);
                }
                let status = i32::from(status);
                slot.body = Body::Failed(status);
                return one(
                    state_record(id, conversion, STATE_FAILED, 0, status, 0),
                    out,
                );
            }
        };
        let flags = ready_flags(&document, io, true)?;
        slot.body = Body::Ready(document);
        one(state_record(id, conversion, STATE_READY, 0, 0, flags), out)
    }

    fn ask(
        document: &DocxDocument,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        match call.op {
            op::META => Self::meta(document, out),
            op::META_TEXT => Self::meta_text(document, call, out),
            op::SECTIONS => {
                let from = min(usize::try_from(call.cursor).map_err(|_| Error::Invalid)?, 1);
                let rows = [Record::FmtSection(stage::FmtSection::new(
                    0,
                    document.section_len(0),
                    0,
                ))];
                page(rows.get(from..).unwrap_or_default(), from, 1, out)
            }
            op::TOC => {
                let total =
                    usize::try_from(document.try_toc_len(io).map_err(|e| document_error(&e))?)
                        .map_err(|_| Error::Invalid)?;
                let from = min(
                    usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
                    total,
                );
                let mut entries = Vec::new();
                entries.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
                for index in from..min(total, from + ROWS - 1) {
                    let index = u32::try_from(index).map_err(|_| Error::Invalid)?;
                    // An entry that cannot be read is an error, not a gap: the
                    // list after it would shift.
                    let entry = document
                        .try_toc_entry(io, index)
                        .map_err(|e| document_error(&e))?
                        .ok_or(Error::Io)?;
                    entries.push((index, entry));
                }
                let mut rows: Vec<Record<'_>> = Vec::new();
                rows.try_reserve(entries.len())
                    .map_err(|_| Error::NoMemory)?;
                for (index, entry) in &entries {
                    rows.push(Record::FmtToc(stage::FmtToc::new(
                        *index,
                        u32::from(entry.level),
                        entry.at.section,
                        entry.at.offset,
                        // A heading starts at its own place: not approximate.
                        0,
                        prefix(&entry.title, 512),
                    )));
                }
                page(&rows, from, total, out)
            }
            op::TOC_TARGET => {
                let entry = document
                    .try_toc_entry(io, call.index)
                    .map_err(|e| document_error(&e))?
                    .ok_or(Error::NotFound)?;
                Self::at(call.document, entry.at, out)
            }
            op::LINKS => Self::links(document, io, call, out),
            op::LINK_TARGET => {
                let place = document
                    .try_link_target(io, call.text)
                    .map_err(|e| document_error(&e))?
                    .ok_or(Error::NotFound)?;
                Self::at(call.document, place, out)
            }
            // A Word document has no cover.
            op::COVER => Err(Error::NotFound),
            op::IMAGE => Self::picture(
                document.picture(io, call.index, call.image_side),
                call.index,
                out,
            ),
            // No page labels or XPointers, and no fonts, in a Word document.
            11..=13 => Err(Error::NotFound),
            16 => one(
                Record::FmtEnd(stage::FmtEnd::new(END_LAST, 0, LIST_GENERATION, 0, None)),
                out,
            ),
            _ => Err(Error::Unsupported),
        }
    }

    fn at(document: u32, place: Locator, out: &mut [u8]) -> Result<usize, Error> {
        one(
            Record::FmtAt(stage::FmtAt::new(
                document,
                place.section,
                place.offset,
                0,
                0,
                0,
            )),
            out,
        )
    }

    fn links(
        document: &DocxDocument,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        if call.section != 0 {
            return one(
                Record::FmtEnd(stage::FmtEnd::new(END_LAST, 0, LIST_GENERATION, 0, None)),
                out,
            );
        }
        let spans = document
            .link_spans(io, call.offset, call.index)
            .map_err(|e| document_error(&e))?;
        let total = spans.len();
        let from = min(
            usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
            total,
        );
        // The rows that can fit a reply: at most `ROWS - 1`, and as many
        // addresses as `ADDRESS_BYTES` hold (the page says where it ends).
        let mut hrefs: Vec<Href> = Vec::new();
        hrefs.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
        let mut bytes = 0;
        for &(_, _, value) in spans.get(from..).unwrap_or_default() {
            if hrefs.len() >= ROWS - 1 || bytes >= ADDRESS_BYTES {
                break;
            }
            let href = match document
                .address(io, value)
                .map_err(|e| document_error(&e))?
            {
                Some(url) => Href::Address(url),
                None => Href::Place(place(value & !cache::EXTERNAL)),
            };
            bytes += href.text().map_or(0, str::len);
            hrefs.push(href);
        }
        let mut rows: Vec<Record<'_>> = Vec::new();
        rows.try_reserve(hrefs.len()).map_err(|_| Error::NoMemory)?;
        for (&(start, end, _), href) in spans.get(from..).unwrap_or_default().iter().zip(&hrefs) {
            rows.push(Record::FmtLink(stage::FmtLink::new(
                start,
                end,
                0,
                href.text().ok_or(Error::Invalid)?,
            )));
        }
        page(&rows, from, total, out)
    }

    fn picture(
        image: Result<Option<ImageRef>, DocumentError>,
        index: u32,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let image = image
            .map_err(|e| document_error(&e))?
            .ok_or(Error::NotFound)?;
        one(
            Record::FmtImage(stage::FmtImage::new(
                &image.path,
                index,
                image.width,
                image.height,
                0,
            )),
            out,
        )
    }

    fn meta(document: &DocxDocument, out: &mut [u8]) -> Result<usize, Error> {
        let m = document.metadata();
        let title = m.title.as_deref().unwrap_or_default();
        let short = prefix(title, 512);
        let language = m.language.as_deref().unwrap_or_default();
        if language.len() > 35 {
            return Err(Error::Invalid);
        }
        let count = |n: usize| u32::try_from(n).map_err(|_| Error::Invalid);
        one(
            Record::FmtMeta(stage::FmtMeta::new(
                short,
                language,
                "",
                "",
                0,
                0,
                document.sections(),
                count(m.authors.len())?,
                count(m.subjects.len())?,
                count(m.description.as_deref().map_or(0, str::len))?,
                u32::from(short.len() < title.len()),
            )),
            out,
        )
    }

    fn meta_text(
        document: &DocxDocument,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let m = document.metadata();
        let index = usize::try_from(call.index).map_err(|_| Error::Invalid)?;
        let kind = call.flags;
        let text = match kind {
            0 => m.title.as_deref(),
            1 => m.authors.get(index).map(String::as_str),
            2 => m.subjects.get(index).map(String::as_str),
            3 => m.description.as_deref(),
            4 => m.series.as_deref(),
            _ => return Err(Error::Invalid),
        }
        .filter(|_| kind == 1 || kind == 2 || index == 0)
        .ok_or(Error::NotFound)?;
        let offset = usize::try_from(call.offset).map_err(|_| Error::Invalid)?;
        let rest = text
            .get(offset..)
            .filter(|_| text.is_char_boundary(offset))
            .ok_or(Error::Invalid)?;
        one(
            Record::FmtText(stage::FmtText::new(
                kind,
                call.index,
                u32::try_from(text.len()).map_err(|_| Error::Invalid)?,
                prefix(rest, TEXT),
            )),
            out,
        )
    }

    fn block(
        document: &DocxDocument,
        io: &dyn Storage,
        conversion: u64,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let at = Locator::new(call.section, call.offset);
        let block = document
            .block_at(io, at)
            .map_err(|e| document_error(&e))?
            .ok_or(Error::NotFound)?;
        let generation = generation(call.section, block.start, conversion);
        if call.cursor != 0 && call.generation != generation {
            return Err(Error::State);
        }
        let mut list = Vec::new();
        records(&block, document, io, &mut list)?;
        let last = document
            .block_at(io, Locator::new(call.section, block.end))
            .map_err(|e| document_error(&e))?
            .is_none();
        fragment(&list, call.cursor, generation, block.end, last, out)
    }
}

/// Removes what a conversion that is not whole left in the cache (the
/// pictures taken out of the archive and decoded are removed with the cache by
/// the host).
fn clear(io: &dyn Storage) {
    for name in [
        cache::BLOCKS,
        cache::INDEX,
        cache::LINKS,
        cache::TOC_INDEX,
        cache::TOC,
        cache::URLS_INDEX,
        cache::URLS,
        cache::MARKS,
        images::LIST,
        crate::document::INFO,
        DONE,
    ] {
        io.remove(name).ok();
    }
}

/// The `flags` of a ready document's state: bit 1 where it has a table of
/// contents, bit 6 where this call made the cache.
fn ready_flags(document: &DocxDocument, io: &dyn Storage, made: bool) -> Result<u32, Error> {
    let toc = document.try_toc_len(io).map_err(|e| document_error(&e))? > 0;
    Ok((u32::from(toc) * FLAG_TOC) | (u32::from(made) * CACHE_MADE))
}

/// Starts: a ready document where the cache is whole (the host removes one
/// that is not), else one to prepare. `made` says the cache is made by the
/// calls to come.
fn start(io: &dyn Storage, call: &stage::FmtCall<'_>) -> Result<(Body, bool), Error> {
    let fit = fit_of(call.image_side, IMAGE_FIT);
    let color = call.flags & OPEN_COLOR != 0;
    if io.size(DONE).is_ok() {
        let document = DocxDocument::native(io, fit, color).map_err(|e| document_error(&e))?;
        return Ok((Body::Ready(document), false));
    }
    if call.flags & OPEN_DETAILS != 0 {
        // Not for showing: the properties are all that is asked, and the book
        // is not converted for them.
        return match DocxDocument::details(io, fit, color) {
            Ok(document) => Ok((Body::Details(document), false)),
            Err(DocumentError::OutOfMemory) => Err(Error::NoMemory),
            Err(error) => Ok((Body::Failed(i32::from(document_error(&error))), false)),
        };
    }
    Ok((Body::Preparing(fit, color), true))
}

fn generation(section: u32, start: u32, conversion: u64) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for part in [section, start, conversion as u32, (conversion >> 32) as u32] {
        for byte in part.to_le_bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// The record list of a block (section 4): header, text parts, runs, maps and
/// a picture.
fn records<'a>(
    block: &'a Block,
    document: &DocxDocument,
    io: &dyn Storage,
    list: &mut Vec<Record<'a>>,
) -> Result<(), Error> {
    let (kind, level) = match block.kind {
        BlockKind::Paragraph => (0, 0),
        BlockKind::Heading(level) => (1, u32::from(level)),
        BlockKind::Preformatted => (2, 0),
        BlockKind::Image => (3, 0),
        BlockKind::TableRow => (4, 0),
        BlockKind::Extended(n) => (5 + u32::from(n), 0),
        _ => return Err(Error::Invalid),
    };
    let parts = block.text.len().div_ceil(PART);
    list.try_reserve(2 + parts + block.runs.len() + block.map.checkpoints().len())
        .map_err(|_| Error::NoMemory)?;
    list.push(Record::FmtBlock(stage::FmtBlock::new(
        kind,
        level,
        u32::from(block.continues) | u32::from(block.continued) << 1,
        block.start,
        block.end,
        u32::from(block.align.bits()),
        block.indent.map_or(i32::MIN, i32::from),
        block.space.map_or(u32::MAX, u32::from),
        block.size.map_or(u32::MAX, u32::from),
        u32::from(block.margins.0),
        u32::from(block.margins.1),
        block.line_height.map_or(u32::MAX, u32::from),
        0,
        0,
        "",
        u32::from(block.drop_cap),
        u32::try_from(block.text.len()).map_err(|_| Error::Invalid)?,
    )));
    let mut rest = block.text.as_str();
    while !rest.is_empty() {
        let part = prefix(rest, PART);
        list.push(Record::FmtTextPart(stage::FmtTextPart::new(part)));
        rest = rest.get(part.len()..).unwrap_or_default();
    }
    for run in &block.runs {
        list.push(Record::FmtRun(stage::FmtRun::new(
            run.start,
            run.end,
            run.style.bits(),
        )));
    }
    for (text, source) in block.map.checkpoints() {
        list.push(Record::FmtMap(stage::FmtMap::new(text, source)));
    }
    if let Some(image) = &block.image {
        // Not decoded yet: the host asks for it (op 18) when the page shows.
        let decoded = document
            .is_decoded(io, image)
            .map_err(|e| document_error(&e))?;
        list.push(Record::FmtImage(stage::FmtImage::new(
            &image.path,
            images::index_of(&image.path).unwrap_or(0),
            image.width,
            image.height,
            u32::from(!decoded),
        )));
    }
    Ok(())
}

/// The fragment of `list` from record `cursor` that fits `out` with its
/// closing `fmt.end`, cut only between records (section 4).
fn fragment(
    list: &[Record<'_>],
    cursor: u32,
    generation: u32,
    next_block: u32,
    last: bool,
    out: &mut [u8],
) -> Result<usize, Error> {
    let from = usize::try_from(cursor).map_err(|_| Error::Invalid)?;
    let rest = list
        .get(from..)
        .filter(|r| !r.is_empty())
        .ok_or(Error::Invalid)?;
    let mut frame: Vec<Record<'_>> = Vec::new();
    frame.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
    let mut take = min(rest.len(), ROWS - 1);
    loop {
        let more = take < rest.len();
        frame.clear();
        frame.extend(rest.iter().take(take).cloned());
        frame.push(Record::FmtEnd(stage::FmtEnd::new(
            if more {
                END_MORE
            } else if last {
                END_LAST
            } else {
                0
            },
            0,
            generation,
            if more {
                u32::try_from(from + take).map_err(|_| Error::Invalid)?
            } else {
                next_block
            },
            None,
        )));
        match stage::reply_encode(&frame, out) {
            Ok(written) => return Ok(written),
            // A record that fits no reply at all: nothing to give up on.
            Err(_) if take <= 1 => return Err(Error::NoSpace),
            Err(_) => take -= 1,
        }
    }
}
