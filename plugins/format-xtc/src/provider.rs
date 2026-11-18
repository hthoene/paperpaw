//! The interface-7 `core.format` provider of XTC and XTCH books
//! (`docs/abi/document.md` section 3): one `fmt.call` in, the op's records out.
//! The book is root 3 (path `""`), the decoded gray pages root 6 (`p<n>.gray`),
//! written when a page is first shown; the host owns the cache's stamp.
//!
//! Not wired to the host yet (see `PORTING-7.md`): no `format.spec`
//! declaration builder in the SDK, Host's `fmt.call` adapter and activation.
use crate::XtcDocument;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::min;
use paperpaw_sdk::document::{Block, BlockKind, Document, DocumentError, Locator, TocEntry};
use paperpaw_sdk::native::{
    Error, Host,
    stage_schemas::core_format_v1::{self as stage, Record},
    storage::{self, Root},
};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

mod op {
    pub const SNIFF: u32 = 1;
    pub const OPEN: u32 = 2;
    pub const CLOSE: u32 = 4;
    pub const META: u32 = 5;
    pub const META_TEXT: u32 = 6;
    pub const SECTIONS: u32 = 7;
    pub const BLOCK: u32 = 8;
    pub const TOC: u32 = 9;
    pub const TOC_TARGET: u32 = 10;
    pub const COVER: u32 = 17;
    pub const IMAGE: u32 = 18;
}

/// Open documents (the host allows 8 elsewhere, 2 on the X4).
pub const MAX_DOCUMENTS: usize = 8;
/// The format's id, as the declaration spells it (`format-xtc.xtc`).
const FORMAT_ID: &str = "format-xtc.xtc";
/// The code a refused allocation travels as through `StorageError`.
const NO_MEMORY: u16 = 0x6e6d;
const TEXT: usize = 1024;
/// Rows of a reply, one of them the closing `fmt.end`.
const ROWS: usize = 64;
const END_LAST: u32 = 1;
/// The `generation` of a paged list (`document.md` section 3: the format's own for the
/// document, never 0): the lists of a document do not change, so one value serves.
const LIST_GENERATION: u32 = 1;
const FLAG_WHOLE_PAGES: u32 = 1;
const FLAG_TOC: u32 = 1 << 1;

/// The book (path `""`) and the cache (any other path) as one storage.
#[derive(Debug)]
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
struct Slot {
    id: u32,
    /// `Err(status)`: the open failed, the id stays until it is closed.
    document: Result<XtcDocument, i32>,
}

/// The XTC format's documents.
#[derive(Debug, Default)]
pub struct XtcProvider {
    slots: Vec<Slot>,
}

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

/// `rows` then the closing `fmt.end` of a page, the most rows that fit `out`.
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
            Err(_) if take <= 1 && !rows.is_empty() => return Err(Error::NoSpace),
            Err(_) if rows.is_empty() => return Err(Error::NoSpace),
            Err(_) => take -= 1,
        }
    }
}

impl XtcProvider {
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
    /// `fmt.call` or names an open document twice, `NotFound` for a document,
    /// page or chapter that is not there, `Unsupported` for the ops an XTC book
    /// has no answer to, `NoSpace` where `out` is too small.
    pub fn call(&mut self, io: &dyn Storage, input: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let mut records = stage::input_decode(input).map_err(|_| Error::Invalid)?;
        let Some(Ok(Record::FmtCall(call))) = records.next() else {
            return Err(Error::Invalid);
        };
        match call.op {
            op::SNIFF => {
                let head = call.arg;
                if head.starts_with(b"XTC\0") || head.starts_with(b"XTCH") {
                    one(Record::FmtYes(stage::FmtYes::new(0)), out)
                } else {
                    stage::reply_encode(&[], out).map_err(|_| Error::NoSpace)
                }
            }
            op::OPEN => self.open(io, &call, out),
            op::CLOSE => {
                if let Some(at) = self.slots.iter().position(|s| s.id == call.document) {
                    self.slots.swap_remove(at);
                }
                stage::reply_encode(&[], out).map_err(|_| Error::NoSpace)
            }
            _ => {
                let document = self.ready(call.document)?;
                Self::ask(document, io, &call, out)
            }
        }
    }

    fn ready(&self, id: u32) -> Result<&XtcDocument, Error> {
        match &self
            .slots
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::NotFound)?
            .document
        {
            Ok(document) => Ok(document),
            Err(_) => Err(Error::State),
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
        let document = XtcDocument::open_native(io).map_err(|e| i32::from(document_error(&e)));
        // Short memory is for the host to try again, not a failed book.
        if matches!(document, Err(status) if status == i32::from(Error::NoMemory)) {
            return Err(Error::NoMemory);
        }
        let record = match &document {
            Ok(d) => stage::FmtState::new(
                call.document,
                0,
                0,
                0,
                FLAG_WHOLE_PAGES | if d.toc_len(io) > 0 { FLAG_TOC } else { 0 },
                call.conversion,
                FORMAT_ID,
            ),
            Err(status) => {
                stage::FmtState::new(call.document, 2, 0, *status, 0, call.conversion, FORMAT_ID)
            }
        };
        let written = one(Record::FmtState(record), out)?;
        self.slots.push(Slot {
            id: call.document,
            document,
        });
        Ok(written)
    }

    fn ask(
        document: &XtcDocument,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        match call.op {
            op::META => Self::meta(document, out),
            op::META_TEXT => Self::meta_text(document, call, out),
            op::SECTIONS => {
                let total = usize::try_from(document.sections()).map_err(|_| Error::Invalid)?;
                let from = min(
                    usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
                    total,
                );
                let mut rows: Vec<Record<'_>> = Vec::new();
                rows.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
                for index in from..min(total, from + ROWS - 1) {
                    let index = u32::try_from(index).map_err(|_| Error::Invalid)?;
                    rows.push(Record::FmtSection(stage::FmtSection::new(
                        index,
                        document.section_len(index),
                        1,
                    )));
                }
                page(&rows, from, total, out)
            }
            op::BLOCK => Self::block(document, io, call, out),
            op::TOC => {
                let total = usize::try_from(document.toc_len(io)).map_err(|_| Error::Invalid)?;
                let from = min(
                    usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
                    total,
                );
                let mut entries: Vec<(u32, TocEntry)> = Vec::new();
                entries.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
                for index in from..min(total, from + ROWS - 1) {
                    let index = u32::try_from(index).map_err(|_| Error::Invalid)?;
                    // A chapter that cannot be read is an error, not a gap: the list
                    // after it would shift.
                    let entry = document
                        .chapter(io, index)
                        .map_err(|e| document_error(&e))?;
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
                        // A chapter starts on its page: not approximate.
                        0,
                        prefix(&entry.title, 512),
                    )));
                }
                page(&rows, from, total, out)
            }
            op::TOC_TARGET => {
                let entry = document.toc_target(io, call.index).ok_or(Error::NotFound)?;
                one(
                    Record::FmtAt(stage::FmtAt::new(
                        call.document,
                        entry.section,
                        entry.offset,
                        0,
                        0,
                        0,
                    )),
                    out,
                )
            }
            op::COVER => Self::picture(document.cover(io), u32::MAX, out),
            op::IMAGE => {
                if call.index >= document.sections() {
                    return Err(Error::NotFound);
                }
                Self::picture(
                    crate::undamaged(document.page(io, call.index)),
                    call.index,
                    out,
                )
            }
            11 | 12 | 13 | 15 => Err(Error::NotFound),
            14 | 16 => one(
                Record::FmtEnd(stage::FmtEnd::new(END_LAST, 0, LIST_GENERATION, 0, None)),
                out,
            ),
            _ => Err(Error::Unsupported),
        }
    }

    fn picture(
        image: Result<Option<paperpaw_sdk::document::ImageRef>, DocumentError>,
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

    fn meta(document: &XtcDocument, out: &mut [u8]) -> Result<usize, Error> {
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
        document: &XtcDocument,
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
        document: &XtcDocument,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let block: Block = document
            .block_at(io, Locator::new(call.section, call.offset))
            .map_err(|e| document_error(&e))?
            .ok_or(Error::NotFound)?;
        // A page is one block of at most a page number or a picture: one fragment.
        if call.cursor != 0 {
            return Err(Error::Invalid);
        }
        let kind = match block.kind {
            BlockKind::Image => 3,
            BlockKind::Paragraph => 0,
            _ => return Err(Error::Invalid),
        };
        let header = stage::FmtBlock::new(
            kind,
            0,
            0,
            block.start,
            block.end,
            u32::from(block.align.bits()),
            i32::MIN,
            u32::MAX,
            u32::MAX,
            0,
            0,
            u32::MAX,
            0,
            0,
            "",
            0,
            u32::try_from(block.text.len()).map_err(|_| Error::Invalid)?,
        );
        let end = Record::FmtEnd(stage::FmtEnd::new(END_LAST, 0, 0, 1, None));
        let mut text = [None, None];
        if !block.text.is_empty() {
            text[0] = Some(Record::FmtTextPart(stage::FmtTextPart::new(&block.text)));
        }
        if let Some(image) = &block.image {
            text[1] = Some(Record::FmtImage(stage::FmtImage::new(
                &image.path,
                call.section,
                image.width,
                image.height,
                0,
            )));
        }
        let mut rows: Vec<Record<'_>> = Vec::new();
        rows.try_reserve(4).map_err(|_| Error::NoMemory)?;
        rows.push(Record::FmtBlock(header));
        rows.extend(text.into_iter().flatten());
        rows.push(end);
        stage::reply_encode(&rows, out).map_err(|_| Error::NoSpace)
    }
}
