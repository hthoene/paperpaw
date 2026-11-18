//! What is made of a book when it is first opened, kept in the book's
//! folder of the plugin (moved and removed with the book): its blocks of
//! text in `blocks.bin`, where they are in `index.bin` (a start, an end
//! and a place in `blocks.bin`, 12 bytes each, in order, so a block is
//! found by halving), its links in `links.bin` (a start, an end, the
//! position it leads to) and its table of contents in `toc.idx` and
//! `toc.bin`, the addresses of its links to the web in `urls.idx` and
//! `urls.bin`, and the places of its bookmarks in `marks.bin` (a hash of the
//! name, a position, 8 bytes each). `done` is written last: a folder
//! without it is made anew. `dir` is the folder's path, or `""` for the
//! native provider's cache (root 6: the names are the whole path).
//!
//! Every allocation can be refused: the writer latches it and the conversion
//! ends with `OutOfMemory`; the readers say it as an error.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, DocumentError, OffsetMap, Run, TextStyle};
use paperpaw_sdk::{Storage, StorageError};

use crate::mem::{self, Refused};

pub(crate) const BLOCKS: &str = "blocks.bin";
pub(crate) const INDEX: &str = "index.bin";
pub(crate) const LINKS: &str = "links.bin";
pub(crate) const TOC_INDEX: &str = "toc.idx";
pub(crate) const TOC: &str = "toc.bin";
pub(crate) const URLS_INDEX: &str = "urls.idx";
pub(crate) const URLS: &str = "urls.bin";
pub(crate) const MARKS: &str = "marks.bin";
pub(crate) const DONE: &str = "done";
/// A link's value with this bit set is the number of an address in
/// `urls.bin`; without it, the hash of a bookmark's name.
pub(crate) const EXTERNAL: u32 = 1 << 31;
/// Most addresses and bookmarks kept.
const MOST_URLS: u32 = 4096;
const MOST_MARKS: u32 = 20_000;

/// The most the cache files may hold (bytes of `blocks.bin`).
const MOST_BLOCKS: usize = 96 * 1024 * 1024;
/// Written to the card when this many bytes wait.
const FLUSH: usize = 6 * 1024;
/// The most a block's text may be: the block's own length field is 16 bits.
const MOST_TEXT: usize = 60_000;
/// Most runs or checkpoints a block holds.
const MOST_PARTS: usize = 4096;

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

/// The path of file `name` in the folder `dir` (`name` alone for `""`).
pub(crate) fn path(dir: &str, name: &str) -> Result<String, Refused> {
    if dir.is_empty() {
        mem::string(name)
    } else {
        mem::formatted(format_args!("{dir}/{name}"))
    }
}

/// What is written, waiting to be put on the card.
#[derive(Debug, Default)]
pub(crate) struct Output {
    blocks: Vec<u8>,
    index: Vec<u8>,
    links: Vec<u8>,
    toc_index: Vec<u8>,
    toc: Vec<u8>,
    urls_index: Vec<u8>,
    urls: Vec<u8>,
    marks: Vec<u8>,
    written: usize,
    toc_written: usize,
    urls_written: usize,
    urls_count: u32,
    marks_count: u32,
    error: Option<StorageError>,
    full: bool,
    /// An allocation was refused: nothing written after it counts.
    oom: bool,
    pub(crate) count: u32,
}

/// `bytes` added to `list`, latching a refusal in `oom`.
fn put(list: &mut Vec<u8>, bytes: &[u8], oom: &mut bool) {
    if !*oom && mem::extend(list, bytes).is_err() {
        *oom = true;
    }
}

impl Output {
    pub(crate) fn failed(&self) -> bool {
        self.error.is_some() || self.full || self.oom
    }

    /// `block` added to the waiting blocks. What waits goes to the card in
    /// folder `dir` first where the block would not fit with it, and the
    /// room for the block is taken once, to the byte: no `Vec` grows by
    /// doubling (old and new copy live together then), and what waits never
    /// passes `FLUSH` and one block.
    pub(crate) fn block(&mut self, storage: &dyn Storage, dir: &str, block: &Block<'_>) {
        if self.oom {
            return;
        }
        let text = block.text.as_bytes();
        let text = &text[..text.len().min(MOST_TEXT)];
        let runs = block.runs.len().min(MOST_PARTS);
        let checks = block.checks.len().min(MOST_PARTS);
        let size = 8 + 12 * usize::from(block.image.is_some()) + text.len() + 6 * (runs + checks);
        if !self.blocks.is_empty() && self.blocks.len() + size > FLUSH {
            self.flush(storage, dir, true);
            if self.error.is_some() || self.oom {
                return;
            }
        }
        let room = FLUSH.max(size);
        if self.blocks.capacity() < room
            && self
                .blocks
                .try_reserve_exact(room - self.blocks.len())
                .is_err()
        {
            self.oom = true;
            return;
        }
        let at = self.written + self.blocks.len();
        if at + text.len() + 1024 > MOST_BLOCKS {
            self.full = true;
            return;
        }
        let oom = &mut self.oom;
        put(&mut self.index, &block.start.to_le_bytes(), oom);
        put(&mut self.index, &block.end.to_le_bytes(), oom);
        put(&mut self.index, &(at as u32).to_le_bytes(), oom);
        self.count += 1;
        let flags =
            u8::from(block.continues) | u8::from(block.continued) << 1 | block.align.bits() << 4;
        let out = &mut self.blocks;
        put(out, &[block.kind.code(), flags], oom);
        put(out, &(text.len() as u16).to_le_bytes(), oom);
        put(
            out,
            &(block.runs.len().min(MOST_PARTS) as u16).to_le_bytes(),
            oom,
        );
        put(
            out,
            &(block.checks.len().min(MOST_PARTS) as u16).to_le_bytes(),
            oom,
        );
        if let Some((record, width, height)) = block.image {
            for n in [record, width, height] {
                put(out, &n.to_le_bytes(), oom);
            }
        }
        put(out, text, oom);
        for &(from, to, style) in block.runs.iter().take(MOST_PARTS) {
            for n in [from, to, style] {
                put(out, &n.to_le_bytes(), oom);
            }
        }
        for &(index, source) in block.checks.iter().take(MOST_PARTS) {
            put(out, &index.to_le_bytes(), oom);
            put(out, &source.to_le_bytes(), oom);
        }
    }

    pub(crate) fn link(&mut self, start: u32, end: u32, position: u32) {
        for n in [start, end, position] {
            put(&mut self.links, &n.to_le_bytes(), &mut self.oom);
        }
    }

    /// Keeps the address `url`; its number for a link.
    pub(crate) fn url(&mut self, url: &str) -> Option<u32> {
        if self.urls_count >= MOST_URLS || self.oom {
            return None;
        }
        let place = (self.urls_written + self.urls.len()) as u32;
        let oom = &mut self.oom;
        put(&mut self.urls_index, &place.to_le_bytes(), oom);
        put(&mut self.urls, &(url.len() as u16).to_le_bytes(), oom);
        put(&mut self.urls, url.as_bytes(), oom);
        self.urls_count += 1;
        Some(self.urls_count - 1)
    }

    /// The bookmark with hash `hash` is at `position`.
    pub(crate) fn mark(&mut self, hash: u32, position: u32) {
        if self.marks_count < MOST_MARKS {
            put(&mut self.marks, &hash.to_le_bytes(), &mut self.oom);
            put(&mut self.marks, &position.to_le_bytes(), &mut self.oom);
            self.marks_count += 1;
        }
    }

    pub(crate) fn toc(&mut self, at: u32, level: u8, title: &str) {
        let title = title.as_bytes();
        let mut len = title.len().min(255);
        while core::str::from_utf8(&title[..len]).is_err() {
            len -= 1;
        }
        let place = (self.toc_written + self.toc.len()) as u32;
        let oom = &mut self.oom;
        put(&mut self.toc_index, &place.to_le_bytes(), oom);
        put(&mut self.toc, &at.to_le_bytes(), oom);
        put(&mut self.toc, &[level, len as u8], oom);
        put(&mut self.toc, &title[..len], oom);
    }

    /// Puts what waits on the card, in the folder `dir`; all of it when
    /// `all`, else once enough waits.
    pub(crate) fn flush(&mut self, storage: &dyn Storage, dir: &str, all: bool) {
        if self.error.is_some() || self.oom {
            return;
        }
        let waiting = self.blocks.len()
            + self.index.len()
            + self.links.len()
            + self.toc.len()
            + self.toc_index.len()
            + self.urls.len()
            + self.urls_index.len()
            + self.marks.len();
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
            (URLS_INDEX, &mut self.urls_index),
            (URLS, &mut self.urls),
            (MARKS, &mut self.marks),
        ] {
            if bytes.is_empty() {
                continue;
            }
            let Ok(file) = path(dir, name) else {
                self.oom = true;
                return;
            };
            result = result.and(storage.append(&file, bytes));
            match name {
                BLOCKS => self.written += bytes.len(),
                TOC => self.toc_written += bytes.len(),
                URLS => self.urls_written += bytes.len(),
                _ => {}
            }
            bytes.clear();
        }
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    pub(crate) fn error(&mut self) -> Option<DocumentError> {
        if self.oom {
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

/// A refused allocation, as the document's error.
fn oom(_: Refused) -> DocumentError {
    DocumentError::OutOfMemory
}

/// `n` bytes of the file `file` at `at` into `buf`: `None` where the file
/// has less (or is not there); an error where the card fails.
fn read_exact(
    storage: &dyn Storage,
    file: &str,
    at: u64,
    buf: &mut [u8],
) -> Result<Option<()>, DocumentError> {
    match storage.read_at(file, at, buf) {
        Ok(n) if n == buf.len() => Ok(Some(())),
        Ok(_) | Err(StorageError::NotFound) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn u32_at(storage: &dyn Storage, file: &str, at: u64) -> Result<Option<u32>, DocumentError> {
    let mut bytes = [0u8; 4];
    Ok(read_exact(storage, file, at, &mut bytes)?.map(|()| u32::from_le_bytes(bytes)))
}

/// The size of `file`: 0 where it is not there.
fn size_of(storage: &dyn Storage, file: &str) -> Result<u64, DocumentError> {
    match storage.size(file) {
        Ok(size) => Ok(size),
        Err(StorageError::NotFound) => Ok(0),
        Err(error) => Err(error.into()),
    }
}

/// How many blocks the cache holds.
pub(crate) fn count(storage: &dyn Storage, dir: &str) -> Result<u32, DocumentError> {
    let index = path(dir, INDEX).map_err(oom)?;
    Ok((size_of(storage, &index)? / 12) as u32)
}

/// The block that has source offset `at`, or the first after it.
pub(crate) fn block_at(
    storage: &dyn Storage,
    dir: &str,
    at: u32,
) -> Result<Option<Loaded>, DocumentError> {
    let index = path(dir, INDEX).map_err(oom)?;
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
    let file = path(dir, BLOCKS).map_err(oom)?;
    let mut head = [0u8; 20];
    let n = match storage.read_at(&file, u64::from(place), &mut head) {
        Ok(n) => n,
        Err(StorageError::NotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if n < 8 {
        return Ok(None);
    }
    let kind = Kind::of(head[0]);
    let flags = head[1];
    let len = usize::from(u16::from_le_bytes([head[2], head[3]]));
    let n_runs = usize::from(u16::from_le_bytes([head[4], head[5]]));
    let n_checks = usize::from(u16::from_le_bytes([head[6], head[7]]));
    // The converter writes no checkpoints (a Word document has no source to
    // map): a block with some is not one of ours, and is not read.
    if n_runs > MOST_PARTS || n_checks != 0 {
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
    let mut body = mem::zeroed(len + n_runs * 6).map_err(oom)?;
    if read_exact(storage, &file, at, &mut body)?.is_none() {
        return Ok(None);
    }
    let text = mem::lossy(&body[..len]).map_err(oom)?;
    let word = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
    let mut runs = Vec::new();
    runs.try_reserve_exact(n_runs)
        .map_err(|_| DocumentError::OutOfMemory)?;
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
    Ok(Some(Loaded {
        kind,
        align: Align::from_bits(flags >> 4),
        continues: flags & 1 != 0,
        continued: flags & 2 != 0,
        start,
        end,
        text,
        runs,
        map: OffsetMap::default(),
        image,
    }))
}

/// A link of the cache: where it starts and ends in the text, and its
/// value (an address's number with [`EXTERNAL`] set, else a bookmark's hash).
pub(crate) type Span = (u32, u32, u32);

/// The links that overlap the source offsets `from..to`, in order (at most
/// 256).
pub(crate) fn link_spans(
    storage: &dyn Storage,
    dir: &str,
    from: u32,
    to: u32,
) -> Result<Vec<Span>, DocumentError> {
    let file = path(dir, LINKS).map_err(oom)?;
    let count = size_of(storage, &file)? / 12;
    // The first link that ends after `from`: links are in order.
    let (mut low, mut high) = (0u64, count);
    while low < high {
        let mid = (low + high) / 2;
        match u32_at(storage, &file, mid * 12 + 4)? {
            Some(end) if end > from => high = mid,
            Some(_) => low = mid + 1,
            None => return Ok(Vec::new()),
        }
    }
    let mut found = Vec::new();
    for i in low..count.min(low + 256) {
        let (Some(start), Some(end), Some(position)) = (
            u32_at(storage, &file, i * 12)?,
            u32_at(storage, &file, i * 12 + 4)?,
            u32_at(storage, &file, i * 12 + 8)?,
        ) else {
            break;
        };
        if start >= to {
            break;
        }
        found
            .try_reserve(1)
            .map_err(|_| DocumentError::OutOfMemory)?;
        found.push((start, end, position));
    }
    Ok(found)
}

/// How many entries the table of contents has.
pub(crate) fn toc_len(storage: &dyn Storage, dir: &str) -> Result<u32, DocumentError> {
    let file = path(dir, TOC_INDEX).map_err(oom)?;
    Ok((size_of(storage, &file)? / 4) as u32)
}

/// Table of contents entry `index`: where it is, its level and title.
pub(crate) fn toc_entry(
    storage: &dyn Storage,
    dir: &str,
    index: u32,
) -> Result<Option<(u32, u8, String)>, DocumentError> {
    let idx = path(dir, TOC_INDEX).map_err(oom)?;
    let Some(place) = u32_at(storage, &idx, u64::from(index) * 4)? else {
        return Ok(None);
    };
    let place = u64::from(place);
    let file = path(dir, TOC).map_err(oom)?;
    let mut head = [0u8; 6];
    if read_exact(storage, &file, place, &mut head)?.is_none() {
        return Ok(None);
    }
    let mut title = mem::zeroed(usize::from(head[5])).map_err(oom)?;
    if read_exact(storage, &file, place + 6, &mut title)?.is_none() {
        return Ok(None);
    }
    Ok(Some((
        u32::from_le_bytes([head[0], head[1], head[2], head[3]]),
        head[4],
        mem::lossy(&title).map_err(oom)?,
    )))
}

/// Address number `index` of the links to the web.
pub(crate) fn url_at(
    storage: &dyn Storage,
    dir: &str,
    index: u32,
) -> Result<Option<String>, DocumentError> {
    let idx = path(dir, URLS_INDEX).map_err(oom)?;
    let Some(place) = u32_at(storage, &idx, u64::from(index) * 4)? else {
        return Ok(None);
    };
    let place = u64::from(place);
    let file = path(dir, URLS).map_err(oom)?;
    let mut head = [0u8; 2];
    if read_exact(storage, &file, place, &mut head)?.is_none() {
        return Ok(None);
    }
    let mut url = mem::zeroed(usize::from(u16::from_le_bytes(head))).map_err(oom)?;
    if read_exact(storage, &file, place + 2, &mut url)?.is_none() {
        return Ok(None);
    }
    Ok(String::from_utf8(url).ok())
}

/// Where the bookmark with hash `hash` is: the first of that name.
pub(crate) fn mark(
    storage: &dyn Storage,
    dir: &str,
    hash: u32,
) -> Result<Option<u32>, DocumentError> {
    let file = path(dir, MARKS).map_err(oom)?;
    let mut chunk = [0u8; 512];
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(&file, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if n < 8 {
            return Ok(None);
        }
        for pair in chunk[..n - n % 8].as_chunks::<8>().0 {
            if u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]) == hash {
                return Ok(Some(u32::from_le_bytes([
                    pair[4], pair[5], pair[6], pair[7],
                ])));
            }
        }
        at += n as u64;
    }
}
