//! The interface-7 `core.format` provider for the crate's formats
//! (`docs/abi/document.md` section 3): one `fmt.call` in, the op's records
//! out, over any parser that is a [`Document`]. A format is an [`Opener`]: how
//! to open its book from the storage of a callback ([`NativeIo`](crate::NativeIo)).
//!
//! Gaps (marked `GAP`): pictures are numbered by their block's start. A
//! document is converted when it is opened (`prepare` has nothing to do): the
//! first chapter of an EPUB takes a while on the first open, as in interface 6.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::min;
use paperpaw_sdk::document::{
    AnnotationKind, Block, BlockKind, Document, DocumentError, ImageRef, LinkKind, Locator,
    PageProgression, VerticalAlign, is_external,
};
use paperpaw_sdk::lang::{Direction, WritingMode};
use paperpaw_sdk::native::{
    Error,
    stage_schemas::core_format_v1::{self as stage, Record},
};
use paperpaw_sdk::{OpenOptions, Storage, StorageError};

use crate::native_io::NO_MEMORY;

mod op {
    pub const SNIFF: u32 = 1;
    pub const OPEN: u32 = 2;
    pub const PREPARE: u32 = 3;
    pub const CLOSE: u32 = 4;
    pub const META: u32 = 5;
    pub const META_TEXT: u32 = 6;
    pub const SECTIONS: u32 = 7;
    pub const BLOCK: u32 = 8;
    pub const TOC: u32 = 9;
    pub const TOC_TARGET: u32 = 10;
    pub const PAGE_LABEL: u32 = 11;
    pub const XPOINTER: u32 = 12;
    pub const LOCATE: u32 = 13;
    pub const LINKS: u32 = 14;
    pub const LINK_TARGET: u32 = 15;
    pub const FONTS: u32 = 16;
    pub const COVER: u32 = 17;
    pub const IMAGE: u32 = 18;
    pub const PROGRESS: u32 = 19;
}

/// Open documents (the host allows 8 elsewhere, 2 on the X4).
pub const MAX_DOCUMENTS: usize = 8;
const TEXT: usize = 1024;
const PART: usize = 3072;
const ROWS: usize = 64;
/// The most faces a book lists (document.md section 6).
const MAX_FACES: usize = 124;
/// The most the copied faces of one book take together on the card.
const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;
const END_LAST: u32 = 1;
const END_MORE: u32 = 2;
const FLAG_WHOLE_PAGES: u32 = 1;
const FLAG_TOC: u32 = 1 << 1;
const FLAG_XPOINTERS: u32 = 1 << 2;
const FLAG_LABELS: u32 = 1 << 4;
const FLAG_RTL: u32 = 1 << 5;

/// How a format opens its book: from the callback's storage, the file's name
/// in its folder (`""` when the book is the file itself) and the options of
/// the call.
pub type Opener = fn(&dyn Storage, &str, &OpenOptions) -> Result<Box<dyn Document>, DocumentError>;

struct Slot {
    id: u32,
    /// `Err(status)`: the open failed, the id stays until it is closed.
    document: Result<Box<dyn Document>, i32>,
    /// The faces found unreadable, too large or over the book's cap: not
    /// inflated again on every `fonts` call.
    bad_faces: Vec<u32>,
}

/// The documents of one format.
pub struct FormatProvider {
    opener: Opener,
    /// The format's id, as the state rows name it.
    format: &'static str,
    /// The folder the parsers keep their conversions in (routed to root 6).
    cache: String,
    sniffs: fn(&[u8]) -> bool,
    slots: Vec<Slot>,
}

impl core::fmt::Debug for FormatProvider {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FormatProvider")
            .field("format", &self.format)
            .field("documents", &self.slots.len())
            .finish_non_exhaustive()
    }
}

fn prefix(text: &str, max: usize) -> &str {
    let mut end = min(max, text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

pub(crate) fn document_error(error: &DocumentError) -> Error {
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

fn empty(out: &mut [u8]) -> Result<usize, Error> {
    stage::reply_encode(&[], out).map_err(|_| Error::NoSpace)
}

fn at_record(document: u32, at: Locator) -> Record<'static> {
    Record::FmtAt(stage::FmtAt::new(document, at.section, at.offset, 0, 0, 0))
}

impl FormatProvider {
    /// A provider with no document open. `cache` is the folder the format's
    /// parsers keep conversions in (it is root 6 in the callbacks).
    #[must_use]
    pub fn new(
        format: &'static str,
        opener: Opener,
        cache: String,
        sniffs: fn(&[u8]) -> bool,
    ) -> Self {
        Self {
            opener,
            format,
            cache,
            sniffs,
            slots: Vec::new(),
        }
    }

    /// The folder the parsers keep conversions in, for the storage of a call.
    #[must_use]
    pub fn cache(&self) -> &str {
        &self.cache
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
    /// place or entry that is not there, `State` after a failed open,
    /// `NoSpace` where `out` is too small, `NoMemory` for a refused allocation.
    pub fn call(&mut self, io: &dyn Storage, input: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let mut records = stage::input_decode(input).map_err(|_| Error::Invalid)?;
        let Some(Ok(Record::FmtCall(call))) = records.next() else {
            return Err(Error::Invalid);
        };
        match call.op {
            op::SNIFF => {
                if (self.sniffs)(call.arg) {
                    one(Record::FmtYes(stage::FmtYes::new(0)), out)
                } else {
                    empty(out)
                }
            }
            op::OPEN => self.open(io, &call, out),
            op::CLOSE => {
                if let Some(at) = self.slots.iter().position(|s| s.id == call.document) {
                    self.slots.swap_remove(at);
                }
                empty(out)
            }
            op::PREPARE => {
                let id = call.document;
                match &self.slot(id)?.document {
                    Ok(_) => one(Record::FmtState(self.state(id, call.conversion, None)), out),
                    Err(status) => one(
                        Record::FmtState(self.state(id, call.conversion, Some(*status))),
                        out,
                    ),
                }
            }
            op::FONTS => {
                let cache = &self.cache;
                let slot = self
                    .slots
                    .iter_mut()
                    .find(|s| s.id == call.document)
                    .ok_or(Error::NotFound)?;
                let Slot {
                    document,
                    bad_faces,
                    ..
                } = slot;
                let document = document.as_ref().map_err(|_| Error::State)?;
                Self::fonts(document.as_ref(), cache, io, &call, bad_faces, out)
            }
            _ => {
                let document = self.ready(call.document)?;
                Self::ask(document, &self.cache, io, &call, out)
            }
        }
    }

    fn slot(&self, id: u32) -> Result<&Slot, Error> {
        self.slots
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::NotFound)
    }

    fn ready(&self, id: u32) -> Result<&dyn Document, Error> {
        match &self.slot(id)?.document {
            Ok(document) => Ok(document.as_ref()),
            Err(_) => Err(Error::State),
        }
    }

    /// The `fmt.state` of document `id`: ready with the flags of what the
    /// book has, or failed with `status`.
    fn state(&self, id: u32, conversion: u64, failed: Option<i32>) -> stage::FmtState<'static> {
        let (state, status, flags) = match failed {
            Some(status) => (2, status, 0),
            None => (0, 0, 0),
        };
        stage::FmtState::new(id, state, 0, status, flags, conversion, self.format)
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
        // Flags of the open: bit 0 the publisher's styles, bit 1 colour.
        let options = OpenOptions::default()
            .publisher_styles(call.flags & 1 != 0)
            .color(call.flags & 2 != 0)
            .image_fit(if call.image_side == 0 {
                paperpaw_sdk::IMAGE_FIT
            } else {
                paperpaw_sdk::image::Fit::side(call.image_side)
            })
            .cover_fit(if call.cover_side == 0 {
                paperpaw_sdk::COVER_FIT
            } else {
                paperpaw_sdk::image::Fit::side(call.cover_side)
            });
        let document =
            (self.opener)(io, call.text, &options).map_err(|e| i32::from(document_error(&e)));
        // Short memory is for the host to try again, not a failed book.
        if matches!(document, Err(status) if status == i32::from(Error::NoMemory)) {
            return Err(Error::NoMemory);
        }
        let record = match &document {
            Ok(d) => {
                let mut flags = 0;
                if d.whole_pages() {
                    flags |= FLAG_WHOLE_PAGES;
                }
                if d.toc_len(io) > 0 {
                    flags |= FLAG_TOC;
                }
                if d.xpointer(io, Locator::START).is_some() {
                    flags |= FLAG_XPOINTERS;
                }
                if d.page_label(io, Locator::START).is_some() {
                    flags |= FLAG_LABELS;
                }
                if matches!(d.metadata().direction, PageProgression::RightToLeft) {
                    flags |= FLAG_RTL;
                }
                stage::FmtState::new(call.document, 0, 0, 0, flags, call.conversion, self.format)
            }
            Err(status) => self.state(call.document, call.conversion, Some(*status)),
        };
        let written = one(Record::FmtState(record), out)?;
        self.slots.push(Slot {
            id: call.document,
            document,
            bad_faces: Vec::new(),
        });
        Ok(written)
    }

    /// A page of the section list.
    fn sections(
        document: &dyn Document,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let total = usize::try_from(document.sections()).map_err(|_| Error::Invalid)?;
        let from = min(
            usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
            total,
        );
        let mut rows: Vec<Record<'_>> = Vec::new();
        rows.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
        for index in from..min(total, from + ROWS - 1) {
            let index = u32::try_from(index).map_err(|_| Error::Invalid)?;
            let whole = u32::from(document.whole_pages());
            rows.push(Record::FmtSection(stage::FmtSection::new(
                index,
                document.section_len(index),
                whole,
            )));
        }
        page(&rows, from, total, out)
    }

    /// A page of the contents.
    fn toc(
        document: &dyn Document,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let total = usize::try_from(document.toc_len(io)).map_err(|_| Error::Invalid)?;
        let from = min(
            usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
            total,
        );
        let mut entries = Vec::new();
        entries.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
        for index in from..min(total, from + ROWS - 1) {
            let index = u32::try_from(index).map_err(|_| Error::Invalid)?;
            // A title that cannot be read fails the call: a gap would shift the list.
            let entry = document.toc_entry(io, index).ok_or(Error::Io)?;
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
                0,
                prefix(&entry.title, 512),
            )));
        }
        page(&rows, from, total, out)
    }

    fn ask(
        document: &dyn Document,
        cache: &str,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let at = Locator::new(call.section, call.offset);
        match call.op {
            op::META => Self::meta(document, out),
            op::META_TEXT => Self::meta_text(document, call, out),
            op::SECTIONS => Self::sections(document, call, out),
            op::BLOCK => Self::block(document, cache, io, call, out),
            op::TOC => Self::toc(document, io, call, out),
            op::TOC_TARGET => {
                let to = document.toc_target(io, call.index).ok_or(Error::NotFound)?;
                one(at_record(call.document, to), out)
            }
            op::PAGE_LABEL => {
                let label = document.page_label(io, at).ok_or(Error::NotFound)?;
                one(
                    Record::FmtLabel(stage::FmtLabel::new(prefix(&label, 32))),
                    out,
                )
            }
            op::XPOINTER => {
                let text = document.xpointer(io, at).ok_or(Error::NotFound)?;
                if text.len() > TEXT {
                    return Err(Error::NotFound);
                }
                one(
                    Record::FmtXpointer(stage::FmtXpointer::new(call.document, &text)),
                    out,
                )
            }
            op::LOCATE => {
                let to = document
                    .locate_xpointer(io, call.text)
                    .ok_or(Error::NotFound)?;
                one(at_record(call.document, to), out)
            }
            op::LINKS => Self::links(document, io, call, out),
            op::LINK_TARGET => {
                let to = document.link_target(io, call.text).ok_or(Error::NotFound)?;
                one(at_record(call.document, to), out)
            }
            op::COVER => {
                let image = document.cover(io).map_err(|e| document_error(&e))?;
                Self::picture(decoded(document, io, image), cache, u32::MAX, out)
            }
            op::IMAGE => {
                // The picture numbered `index`: a page of a whole-page
                // document (its section), else the block that starts there
                // (until the parsers number their pictures).
                let whole = document.whole_pages();
                let at = if whole {
                    Locator::new(call.index, 0)
                } else {
                    Locator::new(call.section, call.index)
                };
                let block = document
                    .block_at(io, at)
                    .map_err(|e| document_error(&e))?
                    .filter(|b| whole || b.start == call.index)
                    .ok_or(Error::NotFound)?;
                Self::picture(decoded(document, io, block.image), cache, call.index, out)
            }
            op::PROGRESS => {
                let ppm = document.progress(at) * 1_000_000.0;
                // 0 to 1,000,000 by construction (progress is clamped).
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to 0..=1e6"
                )]
                let ppm = ppm.clamp(0.0, 1_000_000.0) as u32;
                one(Record::FmtProgress(stage::FmtProgress::new(ppm)), out)
            }
            _ => Err(Error::Unsupported),
        }
    }

    /// A page of the faces embedded in the book. Each is written whole to
    /// the root-6 file `fonts/<index>` (decimal) before it is listed, with the
    /// file's size as `bytes`; a face that cannot be read is listed with 0
    /// bytes, which the reader answers with its own font.
    fn fonts(
        document: &dyn Document,
        cache: &str,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        bad_faces: &mut Vec<u32>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let faces = document.fonts(io);
        let total = faces.len().min(MAX_FACES);
        let from = min(
            usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
            total,
        );
        let mut rows: Vec<Record<'_>> = Vec::new();
        rows.try_reserve(ROWS).map_err(|_| Error::NoMemory)?;
        for (index, face) in faces
            .iter()
            .enumerate()
            .take(total)
            .skip(from)
            .take(ROWS - 1)
        {
            let index = u32::try_from(index).map_err(|_| Error::Invalid)?;
            let bytes = if bad_faces.contains(&index) {
                0
            } else {
                let bytes = Self::face_file(document, cache, io, index)?;
                let kept = Self::within_cap(cache, io, index, bytes)?;
                if kept == 0 {
                    // Found faulty (or over the cap): not read again while
                    // this document is open.
                    let _ = bad_faces.try_reserve(1);
                    if bad_faces.len() < bad_faces.capacity() {
                        bad_faces.push(index);
                    }
                }
                kept
            };
            rows.push(Record::FmtFont(stage::FmtFont::new(
                index,
                u32::from(face.family),
                u32::from(face.bold) | u32::from(face.italic) << 1,
                bytes,
            )));
        }
        page(&rows, from, total, out)
    }

    /// `bytes` of face `index`, or 0 (its file removed) where the faces
    /// before it and it together are more than [`MAX_FONT_BYTES`].
    fn within_cap(cache: &str, io: &dyn Storage, index: u32, bytes: u32) -> Result<u32, Error> {
        if bytes == 0 {
            return Ok(0);
        }
        let mut total = u64::from(bytes);
        for before in 0..index {
            let path = crate::fallible::try_format!("{cache}/fonts/{before}")
                .map_err(|_| Error::NoMemory)?;
            total += io.size(&path).unwrap_or(0);
        }
        if total <= MAX_FONT_BYTES {
            return Ok(bytes);
        }
        let path =
            crate::fallible::try_format!("{cache}/fonts/{index}").map_err(|_| Error::NoMemory)?;
        let _ = io.remove(&path);
        Ok(0)
    }

    /// The size of the root-6 file of face `index`, written first when it is
    /// not there whole (through a temporary name, so one half-written is
    /// never taken for it); 0 where the face cannot be read.
    fn face_file(
        document: &dyn Document,
        cache: &str,
        io: &dyn Storage,
        index: u32,
    ) -> Result<u32, Error> {
        let path =
            crate::fallible::try_format!("{cache}/fonts/{index}").map_err(|_| Error::NoMemory)?;
        if let Ok(size) = io.size(&path)
            && size > 0
        {
            return Ok(u32::try_from(size).unwrap_or(0));
        }
        let dir = crate::fallible::try_format!("{cache}/fonts").map_err(|_| Error::NoMemory)?;
        let _ = io.create_dir(&dir);
        // A document that copies its faces in chunks does (no more than a
        // chunk of the font is held); the others hand the face over whole.
        let mut params = Vec::new();
        params
            .try_reserve_exact(4 + path.len())
            .map_err(|_| Error::NoMemory)?;
        params.extend_from_slice(&index.to_le_bytes());
        params.extend_from_slice(path.as_bytes());
        match document
            .extended(io, crate::document::FONT_COPY, &params)
            .as_deref()
        {
            Some([0, a, b, c, d]) => return Ok(u32::from_le_bytes([*a, *b, *c, *d])),
            Some([2, ..]) => return Err(Error::NoMemory),
            Some([3, ..]) => return Ok(0),
            // Not a plain file (a WOFF), or no such copy: whole, below.
            _ => {}
        }
        let data = match document.font_data(io, index as usize) {
            Ok(data) => data,
            Err(DocumentError::OutOfMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(0),
        };
        let Ok(bytes) = u32::try_from(data.len()) else {
            return Ok(0);
        };
        if bytes == 0 {
            return Ok(0);
        }
        let temp = crate::fallible::try_format!("{path}.tmp").map_err(|_| Error::NoMemory)?;
        if io.write(&temp, &data).is_err() || io.rename(&temp, &path).is_err() {
            let _ = io.remove(&temp);
            return Err(Error::Io);
        }
        Ok(bytes)
    }

    fn picture(
        image: Option<ImageRef>,
        cache: &str,
        index: u32,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let image = image.ok_or(Error::NotFound)?;
        let path = in_cache(&image.path, cache).ok_or(Error::NotFound)?;
        one(
            Record::FmtImage(stage::FmtImage::new(
                path,
                index,
                image.width,
                image.height,
                0,
            )),
            out,
        )
    }

    fn meta(document: &dyn Document, out: &mut [u8]) -> Result<usize, Error> {
        let m = document.metadata();
        let title = m.title.as_deref().unwrap_or_default();
        let short = prefix(title, 512);
        let series = prefix(m.series.as_deref().unwrap_or_default(), 512);
        // A longer value is cut, never refused: a book with a 40 byte language
        // opened in interface 6.
        let index = prefix(m.series_index.as_deref().unwrap_or_default(), 32);
        let language = prefix(m.language.as_deref().unwrap_or_default(), 35);
        let count = |n: usize| u32::try_from(n).map_err(|_| Error::Invalid);
        one(
            Record::FmtMeta(stage::FmtMeta::new(
                short,
                language,
                series,
                index,
                u32::from(matches!(m.direction, PageProgression::RightToLeft)),
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
        document: &dyn Document,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let m = document.metadata();
        let kind = call.flags;
        let index = usize::try_from(call.index).map_err(|_| Error::Invalid)?;
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

    fn links(
        document: &dyn Document,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let all = document.links(io, call.section, call.offset, call.index);
        let from = min(
            usize::try_from(call.cursor).map_err(|_| Error::Invalid)?,
            all.len(),
        );
        let mut rows: Vec<Record<'_>> = Vec::new();
        rows.try_reserve(min(all.len() - from, ROWS - 1))
            .map_err(|_| Error::NoMemory)?;
        for link in all.iter().skip(from).take(ROWS - 1) {
            // An href over 1,024 bytes is not listed (document.md 2.3).
            if link.href.len() > TEXT {
                continue;
            }
            let kind = if is_external(&link.href) {
                1
            } else if matches!(link.kind, LinkKind::NoteRef) {
                2
            } else {
                0
            };
            rows.push(Record::FmtLink(stage::FmtLink::new(
                link.start, link.end, kind, &link.href,
            )));
        }
        page(&rows, from, all.len(), out)
    }

    fn block(
        document: &dyn Document,
        cache: &str,
        io: &dyn Storage,
        call: &stage::FmtCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let at = Locator::new(call.section, call.offset);
        let mut block = document
            .block_at(io, at)
            .map_err(|e| document_error(&e))?
            .ok_or(Error::NotFound)?;
        // The pictures of a book are decoded when a page shows them: the
        // file of this one may not be there yet (`decoded`).
        block.image = decoded(document, io, block.image.take());
        let generation = generation(call.section, block.start, call.conversion);
        if call.cursor != 0 && call.generation != generation {
            return Err(Error::State);
        }
        // The table's column weights, as little-endian u16s: a record borrows them.
        let mut tags = [[0u8; 16]; 16];
        let mut weights = Vec::new();
        if let Some(table) = &block.table {
            weights
                .try_reserve_exact(table.columns.len() * 2)
                .map_err(|_| Error::NoMemory)?;
            for weight in table.columns.iter() {
                weights.extend_from_slice(&weight.to_le_bytes());
            }
        }
        let mut list = Vec::new();
        let image_index = if document.whole_pages() {
            call.section
        } else {
            block.start
        };
        records(&block, cache, &weights, &mut tags, image_index, &mut list)?;
        let last = document
            .block_at(io, Locator::new(call.section, block.end))
            .map_err(|e| document_error(&e))?
            .is_none();
        fragment(&list, call.cursor, generation, block.end, last, out)
    }
}

/// `image` with its cache file made: the document decodes a picture when a page
/// shows it, so a block that comes first has only the name of a file that is
/// not there yet, which the host would not take for a picture. A picture that
/// cannot be decoded (a broken file) stays as it is: the caller leaves it out.
fn decoded(document: &dyn Document, io: &dyn Storage, image: Option<ImageRef>) -> Option<ImageRef> {
    let image = image?;
    Some(document.prepare_image(io, &image).unwrap_or(image))
}

/// `path` as a path in the document cache (root 6), if it lies in `cache`.
fn in_cache<'a>(path: &'a str, cache: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(cache)?;
    let rest = rest.strip_prefix('/')?;
    (!rest.is_empty() && rest.len() <= 256).then_some(rest)
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
            if more { END_MORE } else { 0 },
            u32::try_from(total).map_err(|_| Error::Invalid)?,
            0,
            u32::try_from(next).map_err(|_| Error::Invalid)?,
            None,
        )));
        match stage::reply_encode(&frame, out) {
            Ok(written) => return Ok(written),
            Err(_) if take <= 1 => return Err(Error::NoSpace),
            Err(_) => take -= 1,
        }
    }
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

/// The record list of a block (section 4): header, text parts, runs, maps,
/// image, columns, cells and rubies.
fn records<'a>(
    block: &'a Block,
    cache: &'a str,
    weights: &'a [u8],
    tags: &'a mut [[u8; 16]; 16],
    image_index: u32,
    list: &mut Vec<Record<'a>>,
) -> Result<(), Error> {
    let (first, others) = tags.split_first_mut().ok_or(Error::Invalid)?;
    // The picture, where it is a file of the cache: a picture block the host
    // would get without its picture record is refused as a whole (it checks a
    // picture block for exactly one), and with it the book. A picture that
    // cannot be served goes out as a paragraph with the block's own text (its
    // alt text, if it has one), so one broken picture costs only itself.
    let picture = block
        .image
        .as_ref()
        .and_then(|i| in_cache(&i.path, cache).map(|p| (p, i)));
    let (kind, level) = match block.kind {
        BlockKind::Paragraph => (0, 0),
        BlockKind::Heading(level) => (1, u32::from(level)),
        BlockKind::Preformatted => (2, 0),
        BlockKind::Image if picture.is_none() => (0, 0),
        BlockKind::Image => (3, 0),
        BlockKind::TableRow => (4, 0),
        BlockKind::Extended(n) => (5 + u32::from(n), 0),
        _ => return Err(Error::Invalid),
    };
    let cells = block.table.as_ref().map_or(0, |t| t.cells.len());
    list.try_reserve(
        2 + block.text.len().div_ceil(PART)
            + block.runs.len()
            + block.map.checkpoints().len()
            + cells
            + block.annotations.len(),
    )
    .map_err(|_| Error::NoMemory)?;
    // `dir` and `writing_mode` use the draft-29 numbers.
    list.push(Record::FmtBlock(stage::FmtBlock::new(
        kind,
        level,
        u32::from(block.continues)
            | u32::from(block.continued) << 1
            | block.table.as_ref().map_or(0, |t| {
                u32::from(t.header) << 8 | u32::from(t.first) << 9 | u32::from(t.last) << 10
            }),
        block.start,
        block.end,
        u32::from(block.align.bits()),
        block.indent.map_or(i32::MIN, i32::from),
        block.space.map_or(u32::MAX, u32::from),
        block.size.map_or(u32::MAX, u32::from),
        u32::from(block.margins.0),
        u32::from(block.margins.1),
        block.line_height.map_or(u32::MAX, u32::from),
        match block.dir {
            Direction::Ltr => 1,
            Direction::Rtl => 2,
            _ => 0,
        },
        match block.writing_mode {
            WritingMode::VerticalRl => 1,
            WritingMode::VerticalLr => 2,
            WritingMode::SidewaysRl => 3,
            WritingMode::SidewaysLr => 4,
            _ => 0,
        },
        lang_tag(block.lang, first),
        u32::from(block.drop_cap),
        u32::try_from(block.text.len()).map_err(|_| Error::Invalid)?,
    )));
    // The languages the runs name by their `lang_index` (from 1).
    if let Some(langs) = &block.inline_langs {
        for (lang, buf) in langs.iter().zip(others.iter_mut()) {
            list.push(Record::FmtInlineLang(stage::FmtInlineLang::new(lang_tag(
                *lang, buf,
            ))));
        }
    }
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
    // GAP: the picture's number (`index`, op 18) is the block's start until
    // the parsers number their pictures; a picture outside the cache is left out.
    if let Some((path, image)) = picture {
        list.push(Record::FmtImage(stage::FmtImage::new(
            path,
            image_index,
            image.width,
            image.height,
            0,
        )));
    }
    table_records(block, weights, list);
    ruby_records(block, list);
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
            Err(_) if take <= 1 => return Err(Error::NoSpace),
            Err(_) => take -= 1,
        }
    }
}

/// A language as BCP 47 text (`language[-Script][-Region]`, at most
/// 3 + 1 + 4 + 1 + 3 bytes) written into `buf`; empty where unknown.
fn lang_tag(lang: paperpaw_sdk::Lang, buf: &mut [u8; 16]) -> &str {
    let mut len = 0;
    let code = lang.language();
    let parts = [Some(code.as_str()), lang.script(), lang.region()];
    for part in parts.into_iter().flatten().filter(|p| !p.is_empty()) {
        if len > 0 {
            if let Some(b) = buf.get_mut(len) {
                *b = b'-';
            }
            len += 1;
        }
        let end = len + part.len();
        match buf.get_mut(len..end) {
            Some(to) => to.copy_from_slice(part.as_bytes()),
            None => return "",
        }
        len = end;
    }
    buf.get(..len)
        .and_then(|b| core::str::from_utf8(b).ok())
        .unwrap_or_default()
}

/// The columns and cells of a table row.
fn table_records<'a>(block: &Block, weights: &'a [u8], list: &mut Vec<Record<'a>>) {
    if let Some(table) = &block.table {
        list.push(Record::FmtColumns(stage::FmtColumns::new(weights)));
        for (cell, width) in table.cells.iter().zip(table.weights()) {
            list.push(Record::FmtCell(stage::FmtCell::new(
                cell.end,
                u32::from(cell.span),
                u32::from(cell.rows),
                u32::from(cell.align.bits()),
                match cell.vertical {
                    VerticalAlign::Middle => 1,
                    VerticalAlign::Bottom => 2,
                    _ => 0,
                },
                width,
            )));
        }
    }
}

/// The rubies, phonetics and glosses of a block.
fn ruby_records<'a>(block: &'a Block, list: &mut Vec<Record<'a>>) {
    for note in &block.annotations {
        // 0 ruby, 1 phonetic, 2 gloss, 3 + n for a kind a later build adds.
        let kind = match note.kind {
            AnnotationKind::Ruby => 0,
            AnnotationKind::Phonetic => 1,
            AnnotationKind::Gloss => 2,
            AnnotationKind::Other(n) => 3 + u32::from(n),
            _ => 2,
        };
        list.push(Record::FmtRuby(stage::FmtRuby::new(
            note.start,
            note.end,
            kind,
            u32::from(note.size),
            prefix(&note.text, 1024),
        )));
    }
}
