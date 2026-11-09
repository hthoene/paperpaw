//! What is made of a book when it is first opened, kept in the book's
//! folder of the plugin (moved and removed with the book): its blocks of
//! text in `blocks.bin`, where they are in `index.bin` (a start, an end
//! and a place in `blocks.bin`, 12 bytes each, in order, so a block is
//! found by halving), its links in `links.bin` (a start, an end, the
//! position it leads to) and its table of contents in `toc.idx` and
//! `toc.bin`. `done` is written last: a folder without it is made anew.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, DocumentError, Link, OffsetMap, Run, TextStyle};
use paperpaw_sdk::{Storage, StorageError};

use crate::mem::{self, Refused};

pub(crate) const BLOCKS: &str = "blocks.bin";
pub(crate) const INDEX: &str = "index.bin";
pub(crate) const LINKS: &str = "links.bin";
pub(crate) const TOC_INDEX: &str = "toc.idx";
pub(crate) const TOC: &str = "toc.bin";
pub(crate) const DONE: &str = "done";

/// The most the cache files may hold (bytes of `blocks.bin`).
const MOST_BLOCKS: usize = 96 * 1024 * 1024;
/// Written to the card when this many bytes wait.
const FLUSH: usize = 6 * 1024;
/// The most a block's text may be: the block's own length field is 16 bits.
const MOST_TEXT: usize = 60_000;
/// Most runs or checkpoints a block holds.
const MOST_PARTS: usize = 4096;
/// Longest title of a table of contents entry, in characters, and the most
/// bytes of one.
const TITLE: usize = 120;
const TITLE_BYTES: usize = 255;

/// The cache file `name` of the book's folder `dir`; with no folder (the
/// document cache of the interface-7 provider is the folder) the name itself.
pub(crate) fn at<'a>(dir: &str, name: &'a str) -> Result<Cow<'a, str>, Refused> {
    if dir.is_empty() {
        Ok(Cow::Borrowed(name))
    } else {
        mem::formatted(format_args!("{dir}/{name}")).map(Cow::Owned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Paragraph,
    /// A heading of level 1 to 6.
    Heading(u8),
    Image,
}

impl Kind {
    fn code(self) -> u8 {
        match self {
            Kind::Paragraph => 0,
            Kind::Heading(level) => level.clamp(1, 6),
            Kind::Image => 7,
        }
    }

    fn of(code: u8) -> Kind {
        match code {
            1..=6 => Kind::Heading(code),
            7 => Kind::Image,
            _ => Kind::Paragraph,
        }
    }
}

/// A block to write.
pub(crate) struct Block<'a> {
    pub kind: Kind,
    pub align: Align,
    pub continues: bool,
    pub continued: bool,
    pub start: u32,
    pub end: u32,
    pub text: &'a str,
    /// Text indexes `from..to` in a style (the bits of a `TextStyle`).
    pub runs: &'a [(u16, u16, u16)],
    /// Where the text and the source part from each other: a text index
    /// and the source offset it is at.
    pub checks: &'a [(u16, u32)],
    /// A picture: its record counted from the first picture, and the size
    /// it is decoded to.
    pub image: Option<(u32, u32, u32)>,
}

/// What is written, waiting to be put on the card.
#[derive(Debug, Default)]
pub(crate) struct Output {
    blocks: Vec<u8>,
    index: Vec<u8>,
    links: Vec<u8>,
    toc_index: Vec<u8>,
    toc: Vec<u8>,
    written: usize,
    toc_written: usize,
    error: Option<StorageError>,
    full: bool,
    /// An allocation was refused: nothing more is written.
    refused: bool,
    pub(crate) count: u32,
}

impl Output {
    pub(crate) fn failed(&self) -> bool {
        self.error.is_some() || self.full || self.refused
    }

    /// A failure of the card or of memory while the converter asked the book
    /// about a picture: the conversion stops with it. Anything else is no
    /// reason to stop.
    pub(crate) fn note(&mut self, error: DocumentError) {
        match error {
            DocumentError::OutOfMemory => self.refused = true,
            DocumentError::Storage(error) => self.error = Some(error),
            _ => {}
        }
    }

    /// An allocation of the converter was refused.
    pub(crate) fn refuse(&mut self) {
        self.refused = true;
    }

    pub(crate) fn block(&mut self, block: &Block<'_>) {
        let text = block.text.as_bytes();
        let text = &text[..text.len().min(MOST_TEXT)];
        let at = self.written + self.blocks.len();
        if at + text.len() + 1024 > MOST_BLOCKS {
            self.full = true;
            return;
        }
        // All the room first: what follows cannot be refused half way.
        let room = 20
            + text.len()
            + 6 * (block.runs.len().min(MOST_PARTS) + block.checks.len().min(MOST_PARTS));
        if self.blocks.try_reserve(room).is_err() || self.index.try_reserve(12).is_err() {
            self.refused = true;
            return;
        }
        self.index.extend_from_slice(&block.start.to_le_bytes());
        self.index.extend_from_slice(&block.end.to_le_bytes());
        self.index.extend_from_slice(&(at as u32).to_le_bytes());
        self.count += 1;
        let flags =
            u8::from(block.continues) | u8::from(block.continued) << 1 | block.align.bits() << 4;
        let out = &mut self.blocks;
        out.push(block.kind.code());
        out.push(flags);
        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
        out.extend_from_slice(&(block.runs.len().min(MOST_PARTS) as u16).to_le_bytes());
        out.extend_from_slice(&(block.checks.len().min(MOST_PARTS) as u16).to_le_bytes());
        if let Some((record, width, height)) = block.image {
            for n in [record, width, height] {
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
        out.extend_from_slice(text);
        for &(from, to, style) in block.runs.iter().take(MOST_PARTS) {
            for n in [from, to, style] {
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
        for &(index, source) in block.checks.iter().take(MOST_PARTS) {
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&source.to_le_bytes());
        }
    }

    pub(crate) fn link(&mut self, start: u32, end: u32, position: u32) {
        if self.links.try_reserve(12).is_err() {
            self.refused = true;
            return;
        }
        for n in [start, end, position] {
            self.links.extend_from_slice(&n.to_le_bytes());
        }
    }

    /// An entry of the table of contents at source offset `at`, titled by
    /// the start of `text` (its line ends as spaces).
    pub(crate) fn toc(&mut self, at: u32, level: u8, text: &str) {
        if self.toc.try_reserve(6 + TITLE_BYTES).is_err() || self.toc_index.try_reserve(4).is_err()
        {
            self.refused = true;
            return;
        }
        let place = (self.toc_written + self.toc.len()) as u32;
        self.toc_index.extend_from_slice(&place.to_le_bytes());
        self.toc.extend_from_slice(&at.to_le_bytes());
        self.toc.push(level);
        let len_at = self.toc.len();
        self.toc.push(0);
        let mut len = 0;
        for c in text.chars().take(TITLE) {
            let c = if c == '\n' { ' ' } else { c };
            if len + c.len_utf8() > TITLE_BYTES {
                break;
            }
            let mut buf = [0u8; 4];
            self.toc
                .extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            len += c.len_utf8();
        }
        if let Some(byte) = self.toc.get_mut(len_at) {
            *byte = len as u8;
        }
    }

    /// Puts what waits on the card, in the folder `dir`; all of it when
    /// `all`, else once enough waits.
    pub(crate) fn flush(&mut self, storage: &dyn Storage, dir: &str, all: bool) {
        if self.error.is_some() || self.refused {
            return;
        }
        let waiting = self.blocks.len()
            + self.index.len()
            + self.links.len()
            + self.toc.len()
            + self.toc_index.len();
        if !all && waiting < FLUSH {
            return;
        }
        let mut result = Ok(());
        for (name, bytes) in [
            (BLOCKS, &mut self.blocks),
            (INDEX, &mut self.index),
            (LINKS, &mut self.links),
            (TOC_INDEX, &mut self.toc_index),
            (TOC, &mut self.toc),
        ] {
            if bytes.is_empty() {
                continue;
            }
            let Ok(path) = at(dir, name) else {
                self.refused = true;
                return;
            };
            result = result.and(storage.append(&path, bytes));
            match name {
                BLOCKS => self.written += bytes.len(),
                TOC => self.toc_written += bytes.len(),
                _ => {}
            }
            bytes.clear();
        }
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    pub(crate) fn error(&mut self) -> Option<DocumentError> {
        if self.refused {
            return Some(DocumentError::OutOfMemory);
        }
        if let Some(error) = self.error.take() {
            return Some(error.into());
        }
        self.full
            .then_some(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
                "the book is too long",
            )))
    }
}

/// A block read from the cache.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loaded {
    pub kind: Kind,
    pub align: Align,
    pub continues: bool,
    pub continued: bool,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub runs: Vec<Run>,
    pub map: OffsetMap,
    pub image: Option<(u32, u32, u32)>,
}

/// `buf` filled from `path` at `at`: how many bytes were there. A card that
/// fails is an error, a file that is not there is no bytes.
fn read(
    storage: &dyn Storage,
    path: &str,
    at: u64,
    buf: &mut [u8],
) -> Result<usize, DocumentError> {
    match storage.read_at(path, at, buf) {
        Ok(n) => Ok(n),
        Err(StorageError::NotFound) => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn u32_at(storage: &dyn Storage, path: &str, at: u64) -> Result<Option<u32>, DocumentError> {
    let mut bytes = [0u8; 4];
    Ok((read(storage, path, at, &mut bytes)? == 4).then(|| u32::from_le_bytes(bytes)))
}

/// The size of a cache file, 0 where it is not there.
fn size_of(storage: &dyn Storage, path: &str) -> Result<u64, DocumentError> {
    match storage.size(path) {
        Ok(size) => Ok(size),
        Err(StorageError::NotFound) => Ok(0),
        Err(error) => Err(error.into()),
    }
}

/// How many blocks the cache holds.
pub(crate) fn count(storage: &dyn Storage, dir: &str) -> Result<u32, DocumentError> {
    let path = at(dir, INDEX)?;
    Ok((size_of(storage, &path)? / 12) as u32)
}

/// The block that has source offset `at`, or the first after it; `None`
/// where there is none.
pub(crate) fn block_at(
    storage: &dyn Storage,
    dir: &str,
    at: u32,
) -> Result<Option<Loaded>, DocumentError> {
    let index = self::at(dir, INDEX)?;
    let (mut low, mut high) = (0u64, u64::from(count(storage, dir)?));
    // The first block that ends after `at`.
    while low < high {
        let mid = (low + high) / 2;
        let Some(end) = u32_at(storage, &index, mid * 12 + 4)? else {
            return Ok(None);
        };
        if end > at {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    let entry = low * 12;
    let (Some(start), Some(end), Some(place)) = (
        u32_at(storage, &index, entry)?,
        u32_at(storage, &index, entry + 4)?,
        u32_at(storage, &index, entry + 8)?,
    ) else {
        return Ok(None);
    };
    load(storage, dir, start, end, place)
}

fn load(
    storage: &dyn Storage,
    dir: &str,
    start: u32,
    end: u32,
    place: u32,
) -> Result<Option<Loaded>, DocumentError> {
    let path = self::at(dir, BLOCKS)?;
    let mut head = [0u8; 20];
    let n = read(storage, &path, u64::from(place), &mut head)?;
    if n < 8 {
        return Ok(None);
    }
    let kind = Kind::of(head[0]);
    let flags = head[1];
    let len = usize::from(u16::from_le_bytes([head[2], head[3]]));
    let n_runs = usize::from(u16::from_le_bytes([head[4], head[5]]));
    let n_checks = usize::from(u16::from_le_bytes([head[6], head[7]]));
    if n_runs > MOST_PARTS || n_checks > MOST_PARTS {
        return Ok(None);
    }
    let mut at = u64::from(place) + 8;
    let image = if kind == Kind::Image {
        if n < 20 {
            return Ok(None);
        }
        at += 12;
        let word = |i: usize| u32::from_le_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
        Some((word(8), word(12), word(16)))
    } else {
        None
    };
    let size = len + n_runs * 6 + n_checks * 6;
    let mut body: Vec<u8> = Vec::new();
    body.try_reserve_exact(size).map_err(|_| Refused)?;
    body.resize(size, 0);
    let n = read(storage, &path, at, &mut body)?;
    if n < body.len() {
        return Ok(None);
    }
    let text = mem::lossy(&body[..len])?;
    let word = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
    let mut runs: Vec<Run> = Vec::new();
    runs.try_reserve_exact(n_runs).map_err(|_| Refused)?;
    for i in 0..n_runs {
        let at = len + i * 6;
        let run = Run::new(
            u32::from(word(at)),
            u32::from(word(at + 2)),
            TextStyle::from_bits(u32::from(word(at + 4))),
        );
        if run.start < run.end && run.end as usize <= text.len() {
            runs.push(run);
        }
    }
    let mut map = OffsetMap::default();
    for i in 0..n_checks {
        let at = len + n_runs * 6 + i * 6;
        let source = u32::from_le_bytes([body[at + 2], body[at + 3], body[at + 4], body[at + 5]]);
        map.try_checkpoint(usize::from(word(at)), source)
            .map_err(|_| Refused)?;
    }
    Ok(Some(Loaded {
        kind,
        align: Align::from_bits(flags >> 4),
        continues: flags & 1 != 0,
        continued: flags & 2 != 0,
        start,
        end,
        text,
        runs,
        map,
        image,
    }))
}

/// The links that overlap the source offsets `from..to`, in order, as
/// (start, end, position they lead to).
pub(crate) fn link_spans(
    storage: &dyn Storage,
    dir: &str,
    from: u32,
    to: u32,
) -> Result<Vec<(u32, u32, u32)>, DocumentError> {
    let path = at(dir, LINKS)?;
    let count = size_of(storage, &path)? / 12;
    // The first link that ends after `from`: links are in order.
    let (mut low, mut high) = (0u64, count);
    while low < high {
        let mid = (low + high) / 2;
        match u32_at(storage, &path, mid * 12 + 4)? {
            Some(end) if end > from => high = mid,
            Some(_) => low = mid + 1,
            None => return Ok(Vec::new()),
        }
    }
    let mut found = Vec::new();
    for i in low..count.min(low + 256) {
        let (Some(start), Some(end), Some(position)) = (
            u32_at(storage, &path, i * 12)?,
            u32_at(storage, &path, i * 12 + 4)?,
            u32_at(storage, &path, i * 12 + 8)?,
        ) else {
            break;
        };
        if start >= to {
            break;
        }
        mem::push(&mut found, (start, end, position))?;
    }
    Ok(found)
}

/// The links that overlap the source offsets `from..to`, in order.
pub(crate) fn links(storage: &dyn Storage, dir: &str, from: u32, to: u32) -> Vec<Link> {
    link_spans(storage, dir, from, to)
        .unwrap_or_default()
        .into_iter()
        .map(|(start, end, position)| Link::new(start, end, alloc::format!("#{position}")))
        .collect()
}

/// How many entries the table of contents has.
pub(crate) fn toc_len(storage: &dyn Storage, dir: &str) -> Result<u32, DocumentError> {
    let path = at(dir, TOC_INDEX)?;
    Ok((size_of(storage, &path)? / 4) as u32)
}

/// Table of contents entry `index`: where it is, its level and title.
pub(crate) fn toc_entry(
    storage: &dyn Storage,
    dir: &str,
    index: u32,
) -> Result<Option<(u32, u8, String)>, DocumentError> {
    let list = at(dir, TOC_INDEX)?;
    let Some(place) = u32_at(storage, &list, u64::from(index) * 4)? else {
        return Ok(None);
    };
    let place = u64::from(place);
    let path = at(dir, TOC)?;
    let mut head = [0u8; 6];
    if read(storage, &path, place, &mut head)? < 6 {
        return Ok(None);
    }
    let mut title = [0u8; 255];
    let len = usize::from(head[5]);
    if read(storage, &path, place + 6, &mut title[..len])? < len {
        return Ok(None);
    }
    Ok(Some((
        u32::from_le_bytes([head[0], head[1], head[2], head[3]]),
        head[4],
        mem::lossy(&title[..len])?,
    )))
}
