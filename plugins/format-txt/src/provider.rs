//! The interface-7 `core.format` provider of plain text (`docs/abi/document.md`
//! section 3): one `fmt.call` in, the op's records out, for the host's document
//! service. The book is root 3 (path `""`), the UTF-8 copy of a UTF-16 file
//! root 6 (`text.utf8`, made by `Preparation` in op 3).
//!
//! Not wired to the host yet: the plugin has no `format.spec` declaration
//! builder, no `fmt.option` input (`input_rows` of `core.format`, Kernel's #385)
//! and no activation (Core #944). The call form is complete and tested here.
use crate::TxtDocument;
use crate::preparation::{Preparation, UTF8_CACHE};
use alloc::vec::Vec;
use core::cmp::min;
use paperpaw_sdk::document::{Block, BlockKind, Document, DocumentError, Locator};
use paperpaw_sdk::native::{
    Error, Host,
    stage_schemas::core_format_v1::{self as stage, Record},
    storage::{self, Root},
};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// Operations of `core.format` this plugin answers.
mod op {
    pub const OPEN: u32 = 2;
    pub const PREPARE: u32 = 3;
    pub const CLOSE: u32 = 4;
    pub const META: u32 = 5;
    pub const META_TEXT: u32 = 6;
    pub const SECTIONS: u32 = 7;
    pub const BLOCK: u32 = 8;
    pub const PROGRESS: u32 = 19;
}

/// Open documents (the host allows 8 elsewhere, 2 on the X4).
pub const MAX_DOCUMENTS: usize = 8;
/// The most a `fmt.text` carries, and a `fmt.text_part`.
const TEXT: usize = 1024;
const PART: usize = 3072;
/// Rows of a reply, one of them the closing `fmt.end`.
const ROWS: usize = 64;
const END_LAST: u32 = 1;
const END_MORE: u32 = 2;
const STATE_READY: u32 = 0;
const STATE_MORE: u32 = 1;
const STATE_FAILED: u32 = 2;
/// The format's id, as `crate::FORMAT` spells it.
const FORMAT_ID: &str = "format-txt.txt";
/// The code a refused allocation travels as through `StorageError`.
const NO_MEMORY: u16 = 0x6e6d;
const CACHE_MADE: u32 = 1 << 6;

/// The book and the cache as one storage, plus the one conversion step.
pub trait Io: Storage {
    /// Converts one piece of a UTF-16 book into the cache; `true` when done.
    ///
    /// # Errors
    /// The host's refusal.
    fn convert(&self, preparation: &mut Preparation) -> Result<bool, Error>;
}

/// Roots 3 and 6 of the document being called, through the host.
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
        Error::Unsupported => StorageError::Unsupported,
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

    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

impl Io for HostIo<'_, '_> {
    fn convert(&self, preparation: &mut Preparation) -> Result<bool, Error> {
        preparation.step(self.host)
    }
}

#[derive(Debug)]
enum Body {
    Preparing(Preparation),
    Ready(TxtDocument),
    Failed(i32),
}

#[derive(Debug)]
struct Slot {
    id: u32,
    conversion: u64,
    body: Body,
}

/// The plain text format's documents.
#[derive(Debug, Default)]
pub struct TxtProvider {
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

fn storage_failure(error: StorageError) -> Error {
    document_error(&DocumentError::Storage(error))
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

impl TxtProvider {
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
    /// a place that is not there, `Busy` while it prepares, `Unsupported` for
    /// the ops this format has no answer to (a text has no table of contents,
    /// links, fonts or pictures), `NoSpace` where `out` is too small.
    pub fn call(&mut self, io: &dyn Io, input: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let mut records = stage::input_decode(input).map_err(|_| Error::Invalid)?;
        let Some(Ok(Record::FmtCall(call))) = records.next() else {
            return Err(Error::Invalid);
        };
        match call.op {
            op::OPEN => self.open(io, &call, out),
            op::CLOSE => {
                if let Some(at) = self.slots.iter().position(|s| s.id == call.document) {
                    self.slots.swap_remove(at);
                }
                stage::reply_encode(&[], out).map_err(|_| Error::NoSpace)
            }
            op::PREPARE => self.prepare(io, &call, out),
            op::META => {
                let document = self.ready(call.document)?;
                Self::meta(document, out)
            }
            op::META_TEXT => {
                let document = self.ready(call.document)?;
                Self::meta_text(document, &call, out)
            }
            op::SECTIONS => {
                let document = self.ready(call.document)?;
                Self::sections(document, call.cursor, out)
            }
            op::BLOCK => {
                let conversion = self.slot(call.document)?.conversion;
                let document = self.ready(call.document)?;
                Self::block(document, io, conversion, &call, out)
            }
            op::PROGRESS => {
                let document = self.ready(call.document)?;
                one(
                    Record::FmtProgress(stage::FmtProgress::new(
                        document.progress_ppm(call.offset),
                    )),
                    out,
                )
            }
            1 => stage::reply_encode(&[], out).map_err(|_| Error::NoSpace),
            // A text has no contents, links or fonts (an empty list) and no
            // labels, XPointers, cover or pictures (`ERR_NOT_FOUND`).
            9 | 14 | 16 => {
                self.ready(call.document)?;
                one(Record::FmtEnd(stage::FmtEnd::new(END_LAST, 0, 1, 0, None)), out)
            }
            10..=13 | 15 | 17 | 18 => {
                self.ready(call.document)?;
                Err(Error::NotFound)
            }
            _ => Err(Error::Unsupported),
        }
    }

    fn slot(&self, id: u32) -> Result<&Slot, Error> {
        self.slots
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::NotFound)
    }

    fn ready(&self, id: u32) -> Result<&TxtDocument, Error> {
        match &self.slot(id)?.body {
            Body::Ready(document) => Ok(document),
            Body::Preparing(_) => Err(Error::Busy),
            Body::Failed(_) => Err(Error::State),
        }
    }

    fn open(
        &mut self,
        io: &dyn Io,
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
        let (body, made) = match start(io) {
            Ok(started) => started,
            // Short memory is for the host to try again, not a failed book.
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(error) => (Body::Failed(i32::from(error)), false),
        };
        let record = match &body {
            Body::Ready(_) => state_record(
                call.document,
                call.conversion,
                STATE_READY,
                0,
                0,
                u32::from(made) * CACHE_MADE,
            ),
            Body::Preparing(p) => state_record(
                call.document,
                call.conversion,
                STATE_MORE,
                p.progress_ppm(),
                0,
                0,
            ),
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
        io: &dyn Io,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let at = self
            .slots
            .iter()
            .position(|s| s.id == call.document)
            .ok_or(Error::NotFound)?;
        let slot = self.slots.get_mut(at).ok_or(Error::NotFound)?;
        let (id, conversion) = (slot.id, slot.conversion);
        let record = match &mut slot.body {
            Body::Ready(_) => state_record(id, conversion, STATE_READY, 0, 0, 0),
            Body::Failed(status) => state_record(id, conversion, STATE_FAILED, 0, *status, 0),
            Body::Preparing(preparation) => match io.convert(preparation) {
                Ok(false) => {
                    state_record(id, conversion, STATE_MORE, preparation.progress_ppm(), 0, 0)
                }
                Ok(true) => match TxtDocument::open_converted(io, "", UTF8_CACHE) {
                    Ok(document) => {
                        slot.body = Body::Ready(document);
                        state_record(id, conversion, STATE_READY, 0, 0, CACHE_MADE)
                    }
                    Err(error) if document_error(&error) == Error::NoMemory => {
                        // The copy is whole: the next call opens it again.
                        return Err(Error::NoMemory);
                    }
                    Err(error) => {
                        let status = i32::from(document_error(&error));
                        slot.body = Body::Failed(status);
                        state_record(id, conversion, STATE_FAILED, 0, status, 0)
                    }
                },
                Err(error) => {
                    let status = i32::from(error);
                    slot.body = Body::Failed(status);
                    state_record(id, conversion, STATE_FAILED, 0, status, 0)
                }
            },
        };
        one(record, out)
    }

    fn meta(document: &TxtDocument, out: &mut [u8]) -> Result<usize, Error> {
        let m = document.metadata();
        let title = m.title.as_deref().unwrap_or_default();
        let short = prefix(title, 512);
        let series = prefix(m.series.as_deref().unwrap_or_default(), 512);
        let index = m.series_index.as_deref().unwrap_or_default();
        let language = m.language.as_deref().unwrap_or_default();
        if index.len() > 32 || language.len() > 35 {
            return Err(Error::Invalid);
        }
        let count = |n: usize| u32::try_from(n).map_err(|_| Error::Invalid);
        one(
            Record::FmtMeta(stage::FmtMeta::new(
                short,
                language,
                series,
                index,
                u32::from(matches!(
                    m.direction,
                    paperpaw_sdk::document::PageProgression::RightToLeft
                )),
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
        document: &TxtDocument,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let m = document.metadata();
        let kind = call.flags;
        let index = usize::try_from(call.index).map_err(|_| Error::Invalid)?;
        let text = match kind {
            0 => m.title.as_deref(),
            1 => m.authors.get(index).map(alloc::string::String::as_str),
            2 => m.subjects.get(index).map(alloc::string::String::as_str),
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

    fn sections(document: &TxtDocument, cursor: u32, out: &mut [u8]) -> Result<usize, Error> {
        let section = Record::FmtSection(stage::FmtSection::new(0, document.section_len(0), 0));
        let end = Record::FmtEnd(stage::FmtEnd::new(END_LAST, document.sections(), 1, 0, None));
        if cursor == 0 {
            stage::reply_encode(&[section, end], out)
        } else {
            stage::reply_encode(&[end], out)
        }
        .map_err(|_| Error::NoSpace)
    }

    fn block(
        document: &TxtDocument,
        io: &dyn Io,
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
        records(&block, &mut list)?;
        let last = document
            .block_at(io, Locator::new(call.section, block.end))
            .map_err(|e| document_error(&e))?
            .is_none();
        fragment(&list, call.cursor, generation, block.end, last, out)
    }
}

/// Reads the book's head and starts: a ready document, or the conversion of a
/// UTF-16 book. `made` says the cache was made by this call.
fn start(io: &dyn Io) -> Result<(Body, bool), Error> {
    let size = io.size("").map_err(storage_failure)?;
    let mut bom = [0u8; 2];
    let n = io.read_at("", 0, &mut bom).map_err(storage_failure)?;
    let big = match bom.get(..n) {
        Some([0xff, 0xfe]) => Some(false),
        Some([0xfe, 0xff]) => Some(true),
        _ => None,
    };
    let Some(big) = big else {
        let document = TxtDocument::open(io, "").map_err(|e| document_error(&e))?;
        return Ok((Body::Ready(document), false));
    };
    // The host removes a cache that is not whole, so a copy that is there is.
    if io.size(UTF8_CACHE).is_ok_and(|copy| copy > 0) {
        let document =
            TxtDocument::open_converted(io, "", UTF8_CACHE).map_err(|e| document_error(&e))?;
        return Ok((Body::Ready(document), false));
    }
    Ok((Body::Preparing(Preparation::new(size, big)?), true))
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

/// The record list of a block (section 4): header, text parts, runs, maps.
fn records<'a>(block: &'a Block, list: &mut Vec<Record<'a>>) -> Result<(), Error> {
    let (kind, level) = match block.kind {
        BlockKind::Paragraph => (0, 0),
        BlockKind::Heading(level) => (1, u32::from(level)),
        BlockKind::Preformatted => (2, 0),
        BlockKind::Image => (3, 0),
        BlockKind::TableRow => (4, 0),
        BlockKind::Extended(n) => (5 + u32::from(n), 0),
        _ => return Err(Error::Invalid),
    };
    // Plain text has no direction, writing mode, language or picture.
    if block.image.is_some() || block.inline_langs.is_some() {
        return Err(Error::Invalid);
    }
    let parts = block.text.len().div_ceil(PART);
    list.try_reserve(1 + parts + block.runs.len() + block.map.checkpoints().len())
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
