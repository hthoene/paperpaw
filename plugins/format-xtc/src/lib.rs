//! XTC and XTCH: the pre-rendered page format of the Xteink readers (as
//! made by converters such as `epub2xtc` or `cbz2xtc`).
//!
//! A container (`XTC\0` or `XTCH`) holds a header, optional metadata
//! (title, author, language), optional chapters and an index of pages;
//! each page is a picture of its own:
//!
//! - `XTG\0`: one bit a pixel, row by row (most significant bit left; a
//!   set bit is white);
//! - `XTH\0`: four grays in two bit planes, column by column from the
//!   right, eight rows a byte (most significant bit on top).
//!
//! Each page is a section of one image block, decoded into the gray cache
//! when it is first shown. Pages are decoded in bands of rows, so a page
//! never needs more than about 20 kilobytes, also on the X4. Offsets and
//! sizes from the file are checked against it before they are used, so a
//! damaged or hostile file fails cleanly instead of filling memory or the
//! card.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{
    Align, Block, BlockKind, Document, DocumentError, ImageRef, Locator, Metadata, OffsetMap,
    TocEntry,
};
use paperpaw_sdk::image::{GrayFile, GrayWriter, ImageError, ImageSink};
use paperpaw_sdk::{Storage, StorageError};

/// The book: root 3, whose path is `""`.
const BOOK: &str = "";

pub mod provider;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
/// The interface-7 plugin.
pub mod native_plugin;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

const XTC: &[u8; 4] = b"XTC\0";
const XTCH: &[u8; 4] = b"XTCH";
const XTG: &[u8; 4] = b"XTG\0";
const XTH: &[u8; 4] = b"XTH\0";
const HEADER: usize = 48;
const METADATA: u64 = 256;
const CHAPTER: u64 = 96;
const INDEX_ENTRY: u64 = 16;
const PAGE_HEADER: u64 = 22;
/// Bytes of a gray page (XTH) held at a time: as many rows of both planes
/// as fit, so the planes, read once for each band of rows, are read only
/// a few times for a whole page (6 times for a screen of the X4).
const BAND_BYTES: usize = 16 * 1024;
/// Bytes read from storage at a time.
const WINDOW: usize = 4096;
/// Chapters kept at most.
const MAX_CHAPTERS: u64 = 1000;
/// Pages are pictures for a screen: no side longer than this, and no more
/// pixels than the largest e-paper screens have (13.3", 3200 by 2400),
/// which bounds the time a page takes to decode and its size on the card.
const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: u64 = 8 << 20;

fn malformed(what: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(what))
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[at..at + 8]);
    u64::from_le_bytes(bytes)
}

/// A copy of `text`; an error instead of an abort when memory is short.
fn copy(text: &str) -> Result<String, DocumentError> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| DocumentError::OutOfMemory)?;
    out.push_str(text);
    Ok(out)
}

/// The parts joined; an error instead of an abort when memory is short.
fn joined(parts: &[&str]) -> Result<String, DocumentError> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
        .map_err(|_| DocumentError::OutOfMemory)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

/// `value` in decimal, on the stack.
fn number(value: u32, out: &mut [u8; 10]) -> &str {
    let mut at = out.len();
    let mut rest = value;
    loop {
        at -= 1;
        out[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    core::str::from_utf8(&out[at..]).unwrap_or_default()
}

/// A zero-terminated UTF-8 string of a fixed-size field, trimmed: bytes
/// that are no UTF-8 become U+FFFD.
fn text(field: &[u8]) -> Result<String, DocumentError> {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let mut out = String::new();
    out.try_reserve_exact(end)
        .map_err(|_| DocumentError::OutOfMemory)?;
    for chunk in field[..end].utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            // U+FFFD is three bytes where the invalid run may be one.
            out.try_reserve(3).map_err(|_| DocumentError::OutOfMemory)?;
            out.push('\u{fffd}');
        }
    }
    let trimmed = out.trim();
    if trimmed.len() == out.len() {
        return Ok(out);
    }
    copy(trimmed)
}

/// A page's result with damage as `None`; memory and card failures pass.
fn undamaged<T>(result: Result<T, DocumentError>) -> Result<Option<T>, DocumentError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(DocumentError::Malformed(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `len` zero bytes; an error instead of an abort when memory is short.
fn zeroed(len: usize) -> Result<Vec<u8>, DocumentError> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
        .map_err(|_| DocumentError::OutOfMemory)?;
    buf.resize(len, 0);
    Ok(buf)
}

fn read_exact(
    storage: &dyn Storage,
    path: &str,
    at: u64,
    out: &mut [u8],
) -> Result<(), DocumentError> {
    let mut filled = 0;
    while filled < out.len() {
        let from = at
            .checked_add(filled as u64)
            .ok_or(malformed("offset past the end"))?;
        let n = storage.read_at(path, from, &mut out[filled..])?;
        if n == 0 {
            return Err(malformed("file too short"));
        }
        filled += n;
    }
    Ok(())
}

/// Reads a file through a window, so many small reads near each other
/// cost one read of storage.
struct Window<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    start: u64,
    len: usize,
    buf: Vec<u8>,
}

impl<'a> Window<'a> {
    fn new(storage: &'a dyn Storage, path: &'a str) -> Result<Self, DocumentError> {
        Ok(Self {
            storage,
            path,
            start: 0,
            len: 0,
            buf: zeroed(WINDOW)?,
        })
    }

    /// The byte at `at`; an error past the end.
    fn byte(&mut self, at: u64) -> Result<u8, DocumentError> {
        if at < self.start || at - self.start >= self.len as u64 {
            self.start = at;
            self.len = self.storage.read_at(self.path, at, &mut self.buf)?;
            if self.len == 0 {
                return Err(malformed("file too short"));
            }
        }
        Ok(self.buf[(at - self.start) as usize])
    }
}

/// An open XTC or XTCH book.
#[derive(Debug)]
pub struct XtcDocument {
    /// The size of the file, which the offsets in it must lie within.
    size: u64,
    pages: u32,
    index: u64,
    metadata: Metadata,
    /// Where the chapter records start.
    chapters_at: u64,
    /// Chapters: their record and first page. Titles are read from the
    /// file when they are wanted, so a long table takes little memory.
    chapters: Vec<(u16, u16)>,
    cover: u32,
}

impl XtcDocument {
    /// Opens the book on root 3 (path `""`), its pages decoded into the
    /// document cache (root 6, whose stamp is the host's). A book without a
    /// title in its metadata has none here: the library names it by its file.
    pub fn open_native(storage: &dyn Storage) -> Result<Self, DocumentError> {
        let mut head = [0u8; HEADER];
        read_exact(storage, BOOK, 0, &mut head)?;
        if &head[..4] != XTC && &head[..4] != XTCH {
            return Err(malformed("not an XTC book"));
        }
        let pages = u32::from(u16_at(&head, 6));
        let has_metadata = head[9] != 0;
        let has_chapters = head[11] != 0;
        let metadata_at = u64_at(&head, 16);
        let index = u64_at(&head, 24);
        if pages == 0 {
            return Err(malformed("no pages"));
        }
        let size = storage.size(BOOK)?;
        if index
            .checked_add(u64::from(pages) * INDEX_ENTRY)
            .is_none_or(|end| end > size)
        {
            return Err(malformed("page index past the end"));
        }
        let mut metadata = Metadata::default();
        let mut cover = 0;
        let mut chapter_count = None;
        // Where the chapters start: after the metadata, if there is any.
        let mut chapters_at = HEADER as u64;
        if has_metadata && metadata_at > 0 {
            chapters_at = metadata_at
                .checked_add(METADATA)
                .filter(|&end| end <= size)
                .ok_or(malformed("metadata past the end"))?;
            let mut m = [0u8; METADATA as usize];
            read_exact(storage, BOOK, metadata_at, &mut m)?;
            metadata.title = Some(text(&m[..128])?).filter(|t| !t.is_empty());
            if let Some(author) = Some(text(&m[128..192])?).filter(|a| !a.is_empty()) {
                metadata
                    .authors
                    .try_reserve_exact(1)
                    .map_err(|_| DocumentError::OutOfMemory)?;
                metadata.authors.push(author);
            }
            metadata.language = Some(text(&m[224..240])?).filter(|l| !l.is_empty());
            let page = u32::from(u16_at(&m, 244));
            if page < pages {
                cover = page;
            }
            chapter_count = Some(u64::from(u16_at(&m, 246)));
        }
        let mut chapters = Vec::new();
        if has_chapters {
            // The chapters lie before the page index.
            let room = index.saturating_sub(chapters_at) / CHAPTER;
            let count = chapter_count.unwrap_or(room).min(room).min(MAX_CHAPTERS);
            chapters
                .try_reserve_exact(count as usize)
                .map_err(|_| DocumentError::OutOfMemory)?;
            // Through a window: the first pages of many records in a read.
            let mut window = Window::new(storage, BOOK)?;
            for record in 0..count as u16 {
                let at = chapters_at + u64::from(record) * CHAPTER + 80;
                let start = u16::from_le_bytes([window.byte(at)?, window.byte(at + 1)?]);
                if u32::from(start) < pages {
                    chapters.push((record, start));
                }
            }
        }
        Ok(Self {
            size,
            pages,
            index,
            metadata,
            chapters_at,
            chapters,
            cover,
        })
    }

    /// Chapter `index`, with the reason when it cannot be read.
    pub(crate) fn chapter(
        &self,
        storage: &dyn Storage,
        index: u32,
    ) -> Result<TocEntry, DocumentError> {
        let &(record, page) = self
            .chapters
            .get(index as usize)
            .ok_or(malformed("no such chapter"))?;
        let mut title = [0u8; 80];
        let at = self.chapters_at + u64::from(record) * CHAPTER;
        read_exact(storage, BOOK, at, &mut title)?;
        Ok(TocEntry {
            title: text(&title)?,
            level: 0,
            at: Locator::new(u32::from(page), 0),
        })
    }

    /// The file of the page's cache (root 6) with `extension`.
    fn cached(page: u32, extension: &str) -> Result<String, DocumentError> {
        let mut digits = [0u8; 10];
        let page = number(page, &mut digits);
        joined(&["p", page, ".", extension])
    }

    /// Page `page` as a gray picture in the cache, decoded if needed. A
    /// page that cannot be decoded is marked, so it is not decoded again
    /// each time it is shown (until the book changes, and its cache with
    /// it).
    pub(crate) fn page(&self, storage: &dyn Storage, page: u32) -> Result<ImageRef, DocumentError> {
        let cache = Self::cached(page, "gray")?;
        if GrayFile::open(storage, &cache).is_none() {
            let broken = Self::cached(page, "broken")?;
            if storage.exists(&broken) {
                return Err(malformed("page cannot be decoded"));
            }
            if let Err(error) = self.decode(storage, page, &cache) {
                let _ = storage.remove(&cache);
                // Short memory or a full card may pass; damage stays.
                if matches!(error, DocumentError::Malformed(_)) {
                    let _ = storage.write(&broken, b"");
                }
                return Err(error);
            }
        }
        let file = GrayFile::open(storage, &cache).ok_or(malformed("page"))?;
        Ok(ImageRef {
            path: file.path,
            width: file.width,
            height: file.height,
        })
    }

    /// Decodes page `page` into the gray picture at `cache`.
    fn decode(&self, storage: &dyn Storage, page: u32, cache: &str) -> Result<(), DocumentError> {
        let at = self
            .index
            .checked_add(u64::from(page) * INDEX_ENTRY)
            .ok_or(malformed("page index past the end"))?;
        let mut entry = [0u8; INDEX_ENTRY as usize];
        read_exact(storage, BOOK, at, &mut entry)?;
        let offset = u64_at(&entry, 0);
        let mut writer = GrayWriter::new(storage, cache);
        decode_page(storage, self.size, offset, &mut writer)?;
        writer.finish().map_err(image_error)
    }
}

fn image_error(e: ImageError) -> DocumentError {
    match e {
        ImageError::Io => DocumentError::Storage(StorageError::Io),
        ImageError::OutOfMemory => DocumentError::OutOfMemory,
        _ => malformed("page cannot be decoded"),
    }
}

/// Decodes the page picture at `offset` of the book (`file_size` bytes long)
/// into `sink`.
fn decode_page(
    storage: &dyn Storage,
    file_size: u64,
    offset: u64,
    sink: &mut dyn ImageSink,
) -> Result<(), DocumentError> {
    let data = offset
        .checked_add(PAGE_HEADER)
        .filter(|&data| data <= file_size)
        .ok_or(malformed("page past the end"))?;
    let mut head = [0u8; PAGE_HEADER as usize];
    read_exact(storage, BOOK, offset, &mut head)?;
    let (width, height) = (u32::from(u16_at(&head, 4)), u32::from(u16_at(&head, 6)));
    let size = u64::from(u32_at(&head, 10));
    if width == 0 || height == 0 {
        return Err(malformed("empty page"));
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(malformed("page too large"));
    }
    let magic = &head[..4];
    // The bytes the pixels are read from, all of which must be there.
    let stride = u64::from(width.div_ceil(8));
    let column = u64::from(height.div_ceil(8));
    let plane = size / 2;
    let pixel_bytes = if magic == XTG {
        stride * u64::from(height)
    } else if magic == XTH {
        plane + column * u64::from(width)
    } else {
        return Err(malformed("page format"));
    };
    if data
        .checked_add(pixel_bytes)
        .is_none_or(|end| end > file_size)
    {
        return Err(malformed("page data past the end"));
    }
    let mut window = Window::new(storage, BOOK)?;
    let mut row = zeroed(width as usize)?;
    sink.start(width, height).map_err(image_error)?;
    if magic == XTG {
        // Rows of bits, a set bit white.
        for y in 0..height {
            for x in 0..width {
                let byte = window.byte(data + u64::from(y) * stride + u64::from(x / 8))?;
                let on = byte >> (7 - (x % 8)) & 1;
                row[x as usize] = if on == 1 { 255 } else { 0 };
            }
            sink.row(&row).map_err(image_error)?;
        }
        return Ok(());
    }
    // Two bit planes; in each, columns from the right, eight rows a byte.
    // A band holds some of those bytes of every column in both planes,
    // read in the order of the file, so the window only moves forward.
    let (width, height) = (width as usize, height as usize);
    let groups = height.div_ceil(8);
    let per_band = (BAND_BYTES / (2 * width)).clamp(1, groups);
    let mut band = zeroed(2 * width * per_band)?;
    let mut g0 = 0;
    while g0 < groups {
        let n = per_band.min(groups - g0);
        // Chunk `p * width + c`: plane `p`, the file's column `c`.
        for (i, chunk) in band.chunks_exact_mut(per_band).enumerate() {
            let (p, c) = ((i / width) as u64, (i % width) as u64);
            let base = data + p * plane + c * column + g0 as u64;
            for (j, byte) in chunk[..n].iter_mut().enumerate() {
                *byte = window.byte(base + j as u64)?;
            }
        }
        for y in g0 * 8..((g0 + n) * 8).min(height) {
            let (j, shift) = (y / 8 - g0, 7 - y % 8);
            for (x, out) in row.iter_mut().enumerate() {
                let c = width - 1 - x;
                let high = band[c * per_band + j] >> shift & 1;
                let low = band[(width + c) * per_band + j] >> shift & 1;
                // 00 white, 01 dark gray, 10 light gray, 11 black.
                *out = [255, 85, 170, 0][usize::from(high << 1 | low)];
            }
            sink.row(&row).map_err(image_error)?;
        }
        g0 += n;
    }
    Ok(())
}

impl Document for XtcDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn whole_pages(&self) -> bool {
        true
    }

    fn sections(&self) -> u32 {
        self.pages
    }

    fn section_len(&self, _: u32) -> u32 {
        1
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        if at.section >= self.pages || at.offset >= 1 {
            return Ok(None);
        }
        // A damaged page is named by its number; a failure of memory or of the
        // card is the caller's.
        let image = undamaged(self.page(storage, at.section))?;
        let (kind, text) = match &image {
            Some(_) => (BlockKind::Image, String::new()),
            None => {
                let mut digits = [0u8; 10];
                (
                    BlockKind::Paragraph,
                    copy(number(at.section + 1, &mut digits))?,
                )
            }
        };
        Ok(Some(
            Block::new(kind, 0, 1, text)
                .map(OffsetMap::IDENTITY)
                .align(Align::Center)
                .image(image),
        ))
    }

    fn toc_len(&self, _: &dyn Storage) -> u32 {
        self.chapters.len() as u32
    }

    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        self.chapter(storage, index).ok()
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        undamaged(self.page(storage, self.cover))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec;
    use core::cell::Cell;
    use paperpaw_i18n::{Catalog, check};
    use paperpaw_sdk::{DirEntry, MemoryStorage, StorageError};

    /// Root 3 (the book, path `""`) and root 6 (any other path), as the
    /// provider has them.
    struct Roots(MemoryStorage);

    impl Roots {
        fn new(book: Vec<u8>) -> Self {
            Self(MemoryStorage::new().with("book", book))
        }

        fn map(path: &str) -> String {
            if path.is_empty() {
                String::from("book")
            } else {
                format!("cache/{path}")
            }
        }
    }

    impl Storage for Roots {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(&Self::map(path), offset, buf)
        }

        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.0.size(&Self::map(path))
        }

        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            if path.is_empty() {
                return Err(StorageError::Unsupported);
            }
            self.0.write(&Self::map(path), data)
        }

        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            if path.is_empty() {
                return Err(StorageError::Unsupported);
            }
            self.0.append(&Self::map(path), data)
        }

        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }

        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.0.remove(&Self::map(path))
        }
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        assert!(check::compare(&en, &de).is_empty());
    }

    /// A page header and its data.
    fn page(magic: &[u8; 4], width: u16, height: u16, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(magic);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(data);
        out
    }

    /// An XTC book of `pages` with metadata and one chapter per page.
    fn book(pages: &[Vec<u8>]) -> Vec<u8> {
        let chapters = pages.len() as u64;
        let index = HEADER as u64 + METADATA + CHAPTER * chapters;
        let data = index + INDEX_ENTRY * pages.len() as u64;
        let mut out = Vec::new();
        out.extend_from_slice(XTC);
        out.extend_from_slice(&0x0100u16.to_le_bytes());
        out.extend_from_slice(&(pages.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 1]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(HEADER as u64).to_le_bytes());
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&data.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        let mut meta = vec![0u8; METADATA as usize];
        meta[..5].copy_from_slice(b"Title");
        meta[128..131].copy_from_slice(b"Ann");
        meta[224..226].copy_from_slice(b"de");
        meta[244..246].copy_from_slice(&1u16.to_le_bytes());
        meta[246..248].copy_from_slice(&(chapters as u16).to_le_bytes());
        out.extend_from_slice(&meta);
        for i in 0..chapters {
            let mut c = vec![0u8; CHAPTER as usize];
            let name = format!("Chapter {}", i + 1);
            c[..name.len()].copy_from_slice(name.as_bytes());
            c[80..82].copy_from_slice(&(i as u16).to_le_bytes());
            out.extend_from_slice(&c);
        }
        let mut at = data;
        for p in pages {
            out.extend_from_slice(&at.to_le_bytes());
            out.extend_from_slice(&(p.len() as u32).to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            at += p.len() as u64;
        }
        for p in pages {
            out.extend_from_slice(p);
        }
        out
    }

    /// A storage that counts the reads of its book (root 3) and the bytes
    /// they return.
    struct Counting {
        inner: Roots,
        reads: Cell<usize>,
        bytes: Cell<usize>,
    }

    impl Counting {
        fn new(inner: Roots) -> Self {
            Self {
                inner,
                reads: Cell::new(0),
                bytes: Cell::new(0),
            }
        }

        /// Reads and bytes read since the last call.
        fn take(&self) -> (usize, usize) {
            (self.reads.take(), self.bytes.take())
        }
    }

    impl Storage for Counting {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            let n = self.inner.read_at(path, offset, buf)?;
            if path.is_empty() {
                self.reads.set(self.reads.get() + 1);
                self.bytes.set(self.bytes.get() + n);
            }
            Ok(n)
        }

        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.inner.size(path)
        }

        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.write(path, data)
        }

        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.append(path, data)
        }

        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.inner.list(dir, visit)
        }

        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.inner.remove(path)
        }
    }

    fn rows(storage: &dyn Storage, image: &ImageRef) -> Vec<Vec<u8>> {
        let file = GrayFile::open(storage, &image.path).unwrap();
        (0..file.height)
            .map(|y| {
                let mut row = vec![0u8; file.width as usize];
                assert!(file.read_row(storage, y, &mut row));
                row
            })
            .collect()
    }

    #[test]
    fn reads_pages_metadata_and_chapters() {
        // 1 bit: 10 pixels wide, two rows; set bits are white.
        let xtg = page(XTG, 10, 2, &[0b1000_0000, 0b0100_0000, 0xFF, 0xC0]);
        // 2 bits: 2 wide, 9 tall: columns from the right, 2 bytes each.
        // Right column: row 0 black (11), row 1 dark (01); left column:
        // row 0 light (10), row 8 black (11).
        let plane1 = [0b1000_0000, 0, 0b1000_0000, 0b1000_0000];
        let plane2 = [0b1100_0000, 0, 0, 0b1000_0000];
        let mut planes = plane1.to_vec();
        planes.extend_from_slice(&plane2);
        let xth = page(XTH, 2, 9, &planes);
        let storage = Roots::new(book(&[xtg, xth]));
        let doc = XtcDocument::open_native(&storage).unwrap();
        assert_eq!(doc.metadata().title.as_deref(), Some("Title"));
        assert_eq!(doc.metadata().authors, ["Ann"]);
        assert_eq!(doc.sections(), 2);
        assert_eq!(doc.toc_len(&storage), 2);
        assert_eq!(doc.toc_entry(&storage, 1).unwrap().at, Locator::new(1, 0));

        let first = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
        let image = first.image.unwrap();
        assert_eq!((image.width, image.height), (10, 2));
        let r = rows(&storage, &image);
        assert_eq!(r[0], [255, 0, 0, 0, 0, 0, 0, 0, 0, 255]);
        assert_eq!(r[1], [255; 10]);

        let second = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
        let r = rows(&storage, &second.image.unwrap());
        assert_eq!(r[0], [170, 0]);
        assert_eq!(r[1], [255, 85]);
        assert_eq!(r[8], [0, 255]);
        // The cover is the page the metadata names.
        let cover = doc.cover(&storage).unwrap().unwrap();
        assert_eq!((cover.width, cover.height), (2, 9));
        assert!(
            doc.block_at(&storage, Locator::new(2, 0))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn column_pages_are_read_a_few_times_only() {
        // An X4 screen of four grays, in a pattern that tells pixels apart.
        let (width, height) = (480u32, 800u32);
        let column = height.div_ceil(8);
        let planes: Vec<u8> = (0..2 * width * column)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let gray = |x: u32, y: u32| {
            let bit = |p: u32| {
                let byte = planes[(p * width * column + (width - 1 - x) * column + y / 8) as usize];
                byte >> (7 - y % 8) & 1
            };
            [255, 85, 170, 0][usize::from(bit(0) << 1 | bit(1))]
        };
        let xth = page(XTH, width as u16, height as u16, &planes);
        let storage = Counting::new(Roots::new(book(&[xth])));
        let doc = XtcDocument::open_native(&storage).unwrap();
        storage.take();
        let block = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
        let (_, bytes) = storage.take();
        assert!(bytes < 8 * planes.len(), "{bytes} bytes read");
        let rows = rows(&storage, &block.image.unwrap());
        for (y, row) in rows.iter().enumerate() {
            for (x, &value) in row.iter().enumerate() {
                assert_eq!(value, gray(x as u32, y as u32), "({x}, {y})");
            }
        }
    }

    #[test]
    fn broken_pages_are_not_decoded_again() {
        let broken = page(b"XTZ\0", 8, 1, &[0]);
        let storage = Counting::new(Roots::new(book(&[broken])));
        let doc = XtcDocument::open_native(&storage).unwrap();
        for view in 0..3 {
            storage.take();
            let block = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
            assert!(block.image.is_none());
            let (reads, _) = storage.take();
            assert_eq!(reads > 0, view == 0, "view {view}: {reads} reads");
        }
    }

    #[test]
    fn chapters_are_read_in_few_reads() {
        let pages: Vec<Vec<u8>> = (0..MAX_CHAPTERS).map(|_| page(XTG, 8, 1, &[0])).collect();
        let storage = Counting::new(Roots::new(book(&pages)));
        let doc = XtcDocument::open_native(&storage).unwrap();
        let (reads, _) = storage.take();
        assert!(reads < 100, "{reads} reads");
        assert_eq!(doc.toc_len(&storage), 1000);
        let last = doc.toc_entry(&storage, 999).unwrap();
        assert_eq!(last.title, "Chapter 1000");
        assert_eq!(last.at, Locator::new(999, 0));
        assert!(doc.toc_entry(&storage, 1000).is_none());
    }

    #[test]
    fn oversized_and_truncated_pages_are_refused() {
        let pages = [
            // Wider than any screen.
            page(XTG, 4097, 1, &[0xff; 513]),
            // 512 bytes of pixels promised, 16 there.
            page(XTG, 64, 64, &[0xff; 16]),
            // 4 Gi pixels: hours of decoding and gigabytes on the card.
            page(XTH, u16::MAX, u16::MAX, &[0; 8]),
        ];
        let storage = Roots::new(book(&pages));
        let doc = XtcDocument::open_native(&storage).unwrap();
        for p in 0..3 {
            let block = doc.block_at(&storage, Locator::new(p, 0)).unwrap().unwrap();
            assert!(block.image.is_none(), "page {p}");
            assert_eq!(block.text, format!("{}", p + 1));
            assert!(storage.0.get(&format!("cache/p{p}.gray")).is_none());
        }
    }

    #[test]
    fn offsets_near_the_end_of_u64_fail_cleanly() {
        let pages = [page(XTG, 8, 1, &[0xff]), page(XTG, 8, 1, &[0x00])];
        let good = book(&pages);
        let patched = |at: usize, value: u64| {
            let mut data = good.clone();
            data[at..at + 8].copy_from_slice(&value.to_le_bytes());
            data
        };
        // The page index, the metadata, the first page.
        let index = u64_at(&good, 24) as usize;
        for (what, data) in [
            ("index", patched(24, u64::MAX - 8)),
            ("metadata", patched(16, u64::MAX - 8)),
            ("page", patched(index, u64::MAX - 8)),
        ] {
            let storage = Roots::new(data);
            let Ok(doc) = XtcDocument::open_native(&storage) else {
                assert_ne!(
                    what, "page",
                    "one broken page stopped the book from opening"
                );
                continue;
            };
            assert_eq!(what, "page", "{what} past the end opened");
            let first = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
            assert!(first.image.is_none());
            let second = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
            assert!(second.image.is_some());
        }
    }

    /// Damaged books must fail cleanly: no panic, no endless decoding, no
    /// page larger on the card than the book could describe. Bytes of a
    /// valid book are changed at random (deterministically, so a failure
    /// can be reproduced from its seed), mostly in its structures.
    #[test]
    fn damaged_books_fail_cleanly() {
        struct Rng(u64);
        impl Rng {
            fn next(&mut self) -> u64 {
                self.0 ^= self.0 >> 12;
                self.0 ^= self.0 << 25;
                self.0 ^= self.0 >> 27;
                self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
            }

            fn below(&mut self, n: usize) -> usize {
                (self.next() % n.max(1) as u64) as usize
            }
        }

        let xth = page(XTH, 24, 20, &[0x5a; 2 * 24 * 3]);
        let good = book(&[
            page(XTG, 16, 4, &[0xa5; 8]),
            xth.clone(),
            page(XTG, 9, 3, &[0x0f; 6]),
            xth,
        ]);
        // Header, metadata, chapters and index, then the pages.
        let structures = u64_at(&good, 32) as usize;
        for seed in 1..=2000u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut data = good.clone();
            for _ in 0..1 + rng.below(4) {
                let at = match rng.below(4) {
                    0 => rng.below(data.len()),
                    _ => rng.below(structures),
                };
                data[at] = match rng.below(3) {
                    0 => data[at] ^ 1 << rng.below(8),
                    1 => [0x00, 0xff, 0x7f, 0x80][rng.below(4)],
                    _ => rng.next() as u8,
                };
            }
            if rng.below(8) == 0 {
                data.truncate(rng.below(data.len()));
            }
            let storage = Roots::new(data.clone());
            let Ok(doc) = XtcDocument::open_native(&storage) else {
                continue;
            };
            for p in 0..doc.sections() {
                let block = doc.block_at(&storage, Locator::new(p, 0)).unwrap();
                if let Some(image) = block.and_then(|b| b.image) {
                    // Every pixel comes from at least one bit of the book.
                    let pixels = u64::from(image.width) * u64::from(image.height);
                    assert!(pixels <= 8 * data.len() as u64, "seed {seed}: page {p}");
                }
            }
            for i in 0..doc.toc_len(&storage) {
                let entry = doc.toc_entry(&storage, i);
                assert!(
                    entry.is_none_or(|e| e.at.section < doc.sections()),
                    "seed {seed}"
                );
            }
            let _ = doc.cover(&storage);
        }
    }
}
