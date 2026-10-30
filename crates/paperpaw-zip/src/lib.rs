//! A streaming ZIP reader.
//!
//! Only an index of the central directory is kept in memory: 8 bytes per
//! entry (name hash and directory offset); details are read from storage
//! on lookup. Entries are inflated chunk by chunk, so a large EPUB never has
//! to fit into RAM. Supports the "stored" and
//! "deflate" methods, which cover EPUB and most other ZIP-based formats.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
use paperpaw_platform::{Storage, StorageError};

/// Size of the end-of-central-directory record without comment.
const EOCD_SIZE: u64 = 22;
/// Largest ZIP comment, which may follow the EOCD record.
const MAX_COMMENT: u64 = 0xffff;
/// Chunk size for reading compressed data (several blocks of a card).
const CHUNK: usize = 4096;
/// The deflate history: back-references reach at most this far.
const WINDOW: usize = 32 * 1024;

/// Why an archive could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ZipError {
    Storage(StorageError),
    /// Not a ZIP archive, or a damaged one.
    Invalid(&'static str),
    /// A feature this reader does not implement (encryption, ZIP64, other
    /// compression methods).
    Unsupported(&'static str),
    /// The entry is larger than allowed by the caller.
    TooLarge,
    /// Memory for buffers could not be allocated.
    OutOfMemory,
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::Storage(e) => write!(f, "storage: {e}"),
            ZipError::Invalid(what) => write!(f, "invalid archive: {what}"),
            ZipError::Unsupported(what) => write!(f, "unsupported: {what}"),
            ZipError::TooLarge => f.write_str("entry too large"),
            ZipError::OutOfMemory => f.write_str("not enough memory"),
        }
    }
}

impl core::error::Error for ZipError {}

impl From<StorageError> for ZipError {
    fn from(e: StorageError) -> Self {
        ZipError::Storage(e)
    }
}

/// Compression method of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Method {
    Stored,
    Deflate,
}

/// An entry of the central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub method: Method,
    pub compressed_size: u32,
    pub size: u32,
    /// CRC-32 of the content, checked when it is read to its end.
    crc: u32,
    /// Offset of the local file header.
    header_offset: u32,
    /// Offset of the central directory record.
    record: u32,
}

impl Entry {
    /// Identifies the entry within its archive; see
    /// [`ZipArchive::entry_by_record`].
    pub fn record(&self) -> u32 {
        self.record
    }
}

/// An open ZIP archive.
#[derive(Debug, Clone)]
pub struct ZipArchive {
    path: String,
    /// `(name hash, offset of the central directory record)`, sorted.
    index: Vec<(u32, u32)>,
}

/// FNV-1a, to find entries without keeping their names in memory. ASCII
/// letters count as lowercase, so names that differ only in case share a
/// hash and can be found by either.
fn hash(name: &[u8]) -> u32 {
    name.iter().fold(0x811c_9dc5, |hash, &b| {
        (hash ^ u32::from(b.to_ascii_lowercase())).wrapping_mul(0x0100_0193)
    })
}

/// The CRC-32 of ZIP entries, over `data` continuing `crc`. A byte at a
/// time: its table takes 1 KiB of flash, made when compiling, for half
/// the work of going half a byte at a time.
fn crc32(crc: u32, data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut bit = 0;
            while bit < 8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
                bit += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };
    let mut crc = !crc;
    for &byte in data {
        crc = (crc >> 8) ^ TABLE[((crc ^ u32::from(byte)) & 0xff) as usize];
    }
    !crc
}

/// Size of a central directory record without name, extra and comment.
const RECORD: usize = 46;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_exact(
    storage: &dyn Storage,
    path: &str,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>, ZipError> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
        .map_err(|_| ZipError::OutOfMemory)?;
    buf.resize(len, 0);
    let mut filled = 0;
    while filled < len {
        let n = storage.read_at(path, offset + filled as u64, &mut buf[filled..])?;
        if n == 0 {
            return Err(ZipError::Invalid("unexpected end of file"));
        }
        filled += n;
    }
    Ok(buf)
}

/// A zero-filled buffer, or [`ZipError::OutOfMemory`].
fn zeroed(len: usize) -> Result<Vec<u8>, ZipError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(len)
        .map_err(|_| ZipError::OutOfMemory)?;
    buffer.resize(len, 0);
    Ok(buffer)
}

/// Finds the end-of-central-directory record and returns it, searching
/// backwards from the end of the file in small steps: it usually is the
/// last 22 bytes, but an archive comment may follow it.
///
/// The record taken is the one whose comment ends with the file. Only if
/// there is none (data after the archive, a damaged comment) is the last
/// one taken whose central directory lies before it, so that a signature
/// inside a comment is not mistaken for the record.
fn find_end_record(storage: &dyn Storage, path: &str, size: u64) -> Result<Vec<u8>, ZipError> {
    const STEP: u64 = 1024;
    if size < EOCD_SIZE {
        return Err(ZipError::Invalid("too small"));
    }
    let lowest = size.saturating_sub(EOCD_SIZE + MAX_COMMENT);
    let mut fallback = None;
    let mut end = size;
    while end > lowest {
        let start = end.saturating_sub(STEP).max(lowest);
        // Overlap by a record less one byte, so every record that starts
        // in this step is read whole.
        let len = ((end + EOCD_SIZE - 1).min(size) - start) as usize;
        let window = read_exact(storage, path, start, len)?;
        for i in (0..(end - start) as usize).rev() {
            let Some(record) = window.get(i..i + EOCD_SIZE as usize) else {
                continue;
            };
            if record[..4] != [0x50, 0x4b, 0x05, 0x06] {
                continue;
            }
            let at = start + i as u64;
            let comment_end = at + EOCD_SIZE + u64::from(u16_at(record, 20).unwrap_or(0));
            if comment_end == size {
                return Ok(record.to_vec());
            }
            let cd_offset = u32_at(record, 16).unwrap_or(0);
            let cd_end = u64::from(cd_offset) + u64::from(u32_at(record, 12).unwrap_or(0));
            // ZIP64 keeps the central directory's place elsewhere.
            let before = cd_offset == u32::MAX || cd_end <= at;
            if before && fallback.is_none() {
                fallback = Some(record.to_vec());
            }
        }
        end = start;
    }
    fallback.ok_or(ZipError::Invalid("no end of central directory"))
}

impl ZipArchive {
    /// Reads the central directory of the archive at `path`, entry by
    /// entry, so even large directories need little memory.
    pub fn open(storage: &dyn Storage, path: &str) -> Result<ZipArchive, ZipError> {
        let size = storage.size(path)?;
        let record = find_end_record(storage, path, size)?;
        let truncated = ZipError::Invalid("truncated");
        let count = u16_at(&record, 10).ok_or(truncated.clone())?;
        let cd_size = u32_at(&record, 12).ok_or(truncated.clone())?;
        let cd_offset = u32_at(&record, 16).ok_or(truncated)?;
        if count == 0xffff || cd_offset == 0xffff_ffff {
            return Err(ZipError::Unsupported("ZIP64"));
        }
        let cd_end = u64::from(cd_offset) + u64::from(cd_size);
        if cd_end > size {
            return Err(ZipError::Invalid("central directory out of range"));
        }

        let mut index = Vec::new();
        index
            .try_reserve_exact(usize::from(count))
            .map_err(|_| ZipError::OutOfMemory)?;
        let mut at = u64::from(cd_offset);
        let bad = ZipError::Invalid("bad central directory entry");
        for _ in 0..count {
            if at + RECORD as u64 > cd_end || at > u64::from(u32::MAX) {
                return Err(bad);
            }
            let head = read_exact(storage, path, at, RECORD)?;
            let field = |offset| u16_at(&head, offset).map(u64::from).ok_or(bad.clone());
            if u32_at(&head, 0) != Some(0x0201_4b50) {
                return Err(bad);
            }
            let name_len = field(28)?;
            let name = read_exact(storage, path, at + RECORD as u64, name_len as usize)?;
            // Hashed as the entry is named (see `ZipArchive::record`).
            index.push((hash(String::from_utf8_lossy(&name).as_bytes()), at as u32));
            at += RECORD as u64 + name_len + field(30)? + field(32)?;
        }
        index.sort_unstable();
        Ok(ZipArchive {
            path: String::from(path),
            index,
        })
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// All entries, read from the central directory one at a time (in no
    /// particular order), e.g. to list the images of a comic archive.
    pub fn entries<'a>(
        &'a self,
        storage: &'a dyn Storage,
    ) -> impl Iterator<Item = Result<Entry, ZipError>> + 'a {
        self.index
            .iter()
            .map(move |&(_, offset)| self.entry_by_record(storage, offset))
    }

    /// The entry named `name` (the path inside the archive), read from the
    /// central directory. Without an entry of exactly that name, one whose
    /// name differs only in the case of ASCII letters is taken, as links in
    /// books made on systems that ignore case may not match their files.
    pub fn entry(&self, storage: &dyn Storage, name: &str) -> Result<Option<Entry>, ZipError> {
        let wanted = hash(name.as_bytes());
        let first = self.index.partition_point(|&(h, _)| h < wanted);
        let mut folded = None;
        for &(h, offset) in &self.index[first..] {
            if h != wanted {
                break;
            }
            let (head, found) = self.record(storage, offset)?;
            if found == name {
                return entry(&head, found, offset).map(Some);
            }
            if folded.is_none() && found.eq_ignore_ascii_case(name) {
                folded = Some((head, found, offset));
            }
        }
        folded
            .map(|(head, found, offset)| entry(&head, found, offset))
            .transpose()
    }

    /// The entry whose [`Entry::record`] is `offset`, a compact way to
    /// remember entries without their names.
    pub fn entry_by_record(&self, storage: &dyn Storage, offset: u32) -> Result<Entry, ZipError> {
        let (head, name) = self.record(storage, offset)?;
        entry(&head, name, offset)
    }

    /// The central directory record at `offset`: its fixed part and the
    /// entry's name (invalid UTF-8 replaced).
    fn record(&self, storage: &dyn Storage, offset: u32) -> Result<(Vec<u8>, String), ZipError> {
        let head = read_exact(storage, &self.path, u64::from(offset), RECORD)?;
        if u32_at(&head, 0) != Some(0x0201_4b50) {
            return Err(ZipError::Invalid("bad central directory entry"));
        }
        let name_len = usize::from(u16_at(&head, 28).unwrap_or(0));
        let name = read_exact(
            storage,
            &self.path,
            u64::from(offset) + RECORD as u64,
            name_len,
        )?;
        let name = match String::from_utf8(name) {
            Ok(name) => name,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        Ok((head, name))
    }

    /// Where a stored (uncompressed) entry's bytes are in the archive file,
    /// as `(offset, length)`, to read them in place; `None` for compressed
    /// entries.
    pub fn stored_range(
        &self,
        storage: &dyn Storage,
        entry: &Entry,
    ) -> Result<Option<(u64, u64)>, ZipError> {
        if entry.method != Method::Stored {
            return Ok(None);
        }
        let offset = self.data_offset(storage, entry)?;
        Ok(Some((offset, u64::from(entry.size))))
    }

    /// The path of the archive file.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Offset of an entry's data, after its local header.
    fn data_offset(&self, storage: &dyn Storage, entry: &Entry) -> Result<u64, ZipError> {
        let header = read_exact(storage, &self.path, u64::from(entry.header_offset), 30)?;
        if u32_at(&header, 0) != Some(0x0403_4b50) {
            return Err(ZipError::Invalid("bad local header"));
        }
        let name_len = u16_at(&header, 26).unwrap_or(0);
        let extra_len = u16_at(&header, 28).unwrap_or(0);
        Ok(u64::from(entry.header_offset) + 30 + u64::from(name_len) + u64::from(extra_len))
    }

    /// Streams the uncompressed content of `entry` to `sink` in chunks.
    /// The sink returns `false` to stop early.
    ///
    /// Inflating needs about 10 KiB of decoder state plus a window that
    /// holds the entry, at most 32 KiB (the deflate history); small entries
    /// such as package documents therefore need very little memory.
    ///
    /// Content that turns out longer than the entry's size is an error as
    /// soon as it does, so the window always holds what it refers back to;
    /// content that is shorter, or whose checksum is wrong, is an error at
    /// its end, where the stream would end.
    pub fn stream(
        &self,
        storage: &dyn Storage,
        entry: &Entry,
        sink: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<(), ZipError> {
        let mut reader = self.reader(storage, entry)?;
        loop {
            let chunk = reader.chunk(storage)?;
            if chunk.is_empty() || !sink(chunk) {
                return Ok(());
            }
        }
    }

    /// The uncompressed content of `entry`, to be read a piece at a time
    /// (needing what [`ZipArchive::stream`] needs while it lives).
    pub fn reader(
        &self,
        storage: &dyn Storage,
        entry: &Entry,
    ) -> Result<EntryReader<'_>, ZipError> {
        let offset = self.data_offset(storage, entry)?;
        let remaining = u64::from(entry.compressed_size);
        let input = zeroed(CHUNK.min(remaining as usize).max(1))?;
        let inflate = match entry.method {
            Method::Stored => None,
            Method::Deflate => {
                let mut decoder: Vec<DecompressorOxide> = Vec::new();
                decoder
                    .try_reserve_exact(1)
                    .map_err(|_| ZipError::OutOfMemory)?;
                // Made in place: built as a value first, its 10 KB would
                // be on the stack, deep in the opening of a book.
                decoder.resize_with(1, DecompressorOxide::default);
                // A power of two (required for a wrapping window) that holds
                // the whole entry, or the full deflate history.
                let window_len = (entry.size.min(WINDOW as u32) as usize)
                    .next_power_of_two()
                    .max(64);
                Some((decoder, zeroed(window_len)?))
            }
        };
        Ok(EntryReader {
            path: &self.path,
            offset,
            remaining,
            input,
            start: 0,
            end: 0,
            inflate,
            position: 0,
            ready: 0,
            left: 0,
            done: false,
            size: u64::from(entry.size),
            crc: entry.crc,
            made: 0,
            made_crc: 0,
        })
    }

    /// Reads a whole entry, refusing entries larger than `limit` bytes.
    pub fn read(
        &self,
        storage: &dyn Storage,
        entry: &Entry,
        limit: usize,
    ) -> Result<Vec<u8>, ZipError> {
        if entry.size as usize > limit {
            return Err(ZipError::TooLarge);
        }
        let mut data = Vec::new();
        data.try_reserve_exact(entry.size as usize)
            .map_err(|_| ZipError::OutOfMemory)?;
        let mut too_large = false;
        self.stream(storage, entry, &mut |chunk| {
            if data.len() + chunk.len() > limit {
                too_large = true;
                return false;
            }
            data.extend_from_slice(chunk);
            true
        })?;
        if too_large {
            return Err(ZipError::TooLarge);
        }
        Ok(data)
    }
}

/// The content of an entry read a piece at a time, inflated where it is
/// compressed (see [`ZipArchive::reader`]).
pub struct EntryReader<'a> {
    path: &'a str,
    /// Where the compressed data goes on, and how much of it is left.
    offset: u64,
    remaining: u64,
    /// Compressed data read, of it `start..end` not yet inflated.
    input: Vec<u8>,
    start: usize,
    end: usize,
    /// Inflating: the decoder and its window (the deflate history).
    inflate: Option<(Vec<DecompressorOxide>, Vec<u8>)>,
    /// Where the window is written next.
    position: usize,
    /// Bytes at hand, not yet taken: `left` from `ready` on, in the window
    /// when inflating, else in `input`.
    ready: usize,
    left: usize,
    done: bool,
    /// The entry's size and checksum, and those of the content so far.
    size: u64,
    crc: u32,
    made: u64,
    made_crc: u32,
}

impl fmt::Debug for EntryReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntryReader")
            .field("path", &self.path)
            .field("remaining", &self.remaining)
            .finish_non_exhaustive()
    }
}

impl EntryReader<'_> {
    /// Reads more compressed data.
    fn fill(&mut self, storage: &dyn Storage) -> Result<(), ZipError> {
        let n = (self.remaining as usize).min(self.input.len());
        let read = storage.read_at(self.path, self.offset, &mut self.input[..n])?;
        if read == 0 {
            return Err(ZipError::Invalid("unexpected end of file"));
        }
        self.offset += read as u64;
        self.remaining -= read as u64;
        self.start = 0;
        self.end = read;
        Ok(())
    }

    /// Makes bytes ready unless some are, or the entry is at its end.
    fn more(&mut self, storage: &dyn Storage) -> Result<(), ZipError> {
        while self.left == 0 && !self.done {
            if self.inflate.is_none() {
                if self.remaining == 0 {
                    self.done = true;
                } else {
                    self.fill(storage)?;
                    (self.ready, self.left) = (0, self.end);
                    self.made += self.end as u64;
                    self.made_crc = crc32(self.made_crc, &self.input[..self.end]);
                }
                continue;
            }
            if self.start == self.end && self.remaining > 0 {
                self.fill(storage)?;
            }
            let Some((decoder, window)) = &mut self.inflate else {
                break;
            };
            let flags = if self.remaining > 0 {
                inflate_flags::TINFL_FLAG_HAS_MORE_INPUT
            } else {
                0
            };
            let (status, consumed, written) = decompress(
                &mut decoder[0],
                &self.input[self.start..self.end],
                window,
                self.position,
                flags,
            );
            self.start += consumed;
            (self.ready, self.left) = (self.position, written);
            self.made += written as u64;
            if self.made > self.size {
                return Err(ZipError::Invalid("entry longer than its size"));
            }
            self.made_crc = crc32(
                self.made_crc,
                &window[self.position..self.position + written],
            );
            self.position = (self.position + written) & (window.len() - 1);
            match status {
                TINFLStatus::Done => self.done = true,
                TINFLStatus::HasMoreOutput => {}
                TINFLStatus::NeedsMoreInput if self.remaining > 0 || self.start < self.end => {}
                TINFLStatus::NeedsMoreInput => {
                    return Err(ZipError::Invalid("truncated deflate data"));
                }
                _ => return Err(ZipError::Invalid("corrupt deflate data")),
            }
        }
        if self.left == 0 && self.done {
            if self.made != self.size {
                return Err(ZipError::Invalid("entry shorter than its size"));
            }
            if self.made_crc != self.crc {
                return Err(ZipError::Invalid("checksum mismatch"));
            }
        }
        Ok(())
    }

    /// The bytes at hand.
    fn at_hand(&self, n: usize) -> &[u8] {
        let range = self.ready..self.ready + n;
        match &self.inflate {
            Some((_, window)) => &window[range],
            None => &self.input[range],
        }
    }

    /// The next bytes of the entry, as many as are at hand; empty at its
    /// end.
    pub fn chunk(&mut self, storage: &dyn Storage) -> Result<&[u8], ZipError> {
        self.more(storage)?;
        let n = core::mem::take(&mut self.left);
        Ok(self.at_hand(n))
    }

    /// Reads the next bytes into `out`; how many, 0 at the end.
    pub fn read(&mut self, storage: &dyn Storage, out: &mut [u8]) -> Result<usize, ZipError> {
        self.more(storage)?;
        let n = self.left.min(out.len());
        out[..n].copy_from_slice(self.at_hand(n));
        self.ready += n;
        self.left -= n;
        Ok(n)
    }
}

/// The entry described by the central directory record `head` (see
/// [`ZipArchive::record`]).
fn entry(head: &[u8], name: String, record: u32) -> Result<Entry, ZipError> {
    let bad = ZipError::Invalid("bad central directory entry");
    let field = |at| u32_at(head, at).ok_or(bad.clone());
    let flags = u16_at(head, 8).ok_or(bad.clone())?;
    let method = match u16_at(head, 10).ok_or(bad.clone())? {
        0 => Method::Stored,
        8 => Method::Deflate,
        _ => return Err(ZipError::Unsupported("compression method")),
    };
    if flags & 1 != 0 {
        return Err(ZipError::Unsupported("encrypted entries"));
    }
    let (compressed_size, size) = (field(20)?, field(24)?);
    // Stored content is read by either size.
    if method == Method::Stored && compressed_size != size {
        return Err(bad);
    }
    Ok(Entry {
        name,
        method,
        compressed_size,
        size,
        crc: field(16)?,
        header_offset: field(42)?,
        record,
    })
}

/// Builds ZIP archives in memory, for tests of ZIP-based formats.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Collects files and writes a ZIP archive. `deflate` compresses with
    /// the given level; otherwise entries are stored.
    #[derive(Debug, Default)]
    pub struct ZipBuilder {
        files: Vec<(String, Vec<u8>, bool)>,
    }

    impl ZipBuilder {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn stored(mut self, name: &str, data: impl Into<Vec<u8>>) -> Self {
            self.files.push((String::from(name), data.into(), false));
            self
        }

        pub fn deflated(mut self, name: &str, data: impl Into<Vec<u8>>) -> Self {
            self.files.push((String::from(name), data.into(), true));
            self
        }

        pub fn build(&self) -> Vec<u8> {
            let mut out = Vec::new();
            let mut central = Vec::new();
            for (name, data, deflate) in &self.files {
                let payload = if *deflate {
                    miniz_oxide::deflate::compress_to_vec(data, 6)
                } else {
                    data.clone()
                };
                let method: u16 = if *deflate { 8 } else { 0 };
                let crc = super::crc32(0, data).to_le_bytes();
                let offset = out.len() as u32;
                let mut header = Vec::new();
                header.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
                header.extend_from_slice(&20u16.to_le_bytes());
                header.extend_from_slice(&0u16.to_le_bytes());
                header.extend_from_slice(&method.to_le_bytes());
                header.extend_from_slice(&[0; 4]); // time, date
                header.extend_from_slice(&crc);
                header.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                header.extend_from_slice(&(data.len() as u32).to_le_bytes());
                header.extend_from_slice(&(name.len() as u16).to_le_bytes());
                header.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&header);
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&payload);

                central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
                central.extend_from_slice(&20u16.to_le_bytes());
                central.extend_from_slice(&20u16.to_le_bytes());
                central.extend_from_slice(&0u16.to_le_bytes());
                central.extend_from_slice(&method.to_le_bytes());
                central.extend_from_slice(&[0; 4]); // time, date
                central.extend_from_slice(&crc);
                central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                central.extend_from_slice(&(data.len() as u32).to_le_bytes());
                central.extend_from_slice(&(name.len() as u16).to_le_bytes());
                central.extend_from_slice(&[0; 12]); // extra, comment, disk, attrs
                central.extend_from_slice(&offset.to_le_bytes());
                central.extend_from_slice(name.as_bytes());
            }
            let cd_offset = out.len() as u32;
            out.extend_from_slice(&central);
            out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            let count = self.files.len() as u16;
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&(central.len() as u32).to_le_bytes());
            out.extend_from_slice(&cd_offset.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::ZipBuilder;
    use super::*;
    use alloc::borrow::ToOwned;
    use alloc::string::ToString;
    use paperpaw_platform::DirEntry;

    /// A single-file storage for tests.
    struct OneFile(Vec<u8>);

    impl Storage for OneFile {
        fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            let start = (offset as usize).min(self.0.len());
            let n = buf.len().min(self.0.len() - start);
            buf[..n].copy_from_slice(&self.0[start..start + n]);
            Ok(n)
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Ok(self.0.len() as u64)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn reads_stored_and_deflated_entries() {
        let big: String = (0..20_000)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        let zip = ZipBuilder::new()
            .stored("mimetype", "application/epub+zip")
            .deflated("OEBPS/big.txt", big.clone())
            .build();
        let storage = OneFile(zip);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.entry(&storage, "missing").unwrap(), None);

        let mimetype = archive.entry(&storage, "mimetype").unwrap().unwrap();
        assert_eq!(
            archive.read(&storage, &mimetype, 100).unwrap(),
            b"application/epub+zip"
        );
        let entry = archive.entry(&storage, "OEBPS/big.txt").unwrap().unwrap();
        let entry = &entry;
        assert_eq!(entry.method, Method::Deflate);
        assert!(entry.compressed_size < entry.size);
        let data = archive.read(&storage, entry, 1 << 20).unwrap();
        assert_eq!(data, big.as_bytes());
        assert_eq!(archive.read(&storage, entry, 100), Err(ZipError::TooLarge));

        // Streaming can stop early.
        let mut seen = 0;
        archive
            .stream(&storage, entry, &mut |chunk| {
                seen += chunk.len();
                false
            })
            .unwrap();
        assert!(seen > 0 && seen <= WINDOW);
    }

    #[test]
    fn entries_read_a_piece_at_a_time() {
        // Longer than the window: it wraps while read.
        let big: Vec<u8> = (0..100_000u32)
            .map(|i| ((i * 7 % 251) ^ (i / 1000)) as u8)
            .collect();
        let zip = ZipBuilder::new()
            .stored("a.bin", big.clone())
            .deflated("b.bin", big.clone())
            .build();
        let storage = OneFile(zip);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        for name in ["a.bin", "b.bin"] {
            let entry = archive.entry(&storage, name).unwrap().unwrap();
            let mut reader = archive.reader(&storage, &entry).unwrap();
            let mut out = Vec::new();
            // Pieces of odd sizes, as a decoder asks for them.
            for size in [1usize, 7, 4096, 333].iter().cycle() {
                let mut piece = alloc::vec![0u8; *size];
                let n = reader.read(&storage, &mut piece).unwrap();
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&piece[..n]);
            }
            assert_eq!(out, big, "{name}");
            assert_eq!(reader.read(&storage, &mut [0; 8]).unwrap(), 0);
        }
    }

    /// Where the central directory records of `zip` start.
    fn records(zip: &[u8]) -> Vec<usize> {
        zip.windows(4)
            .enumerate()
            .filter(|(_, w)| *w == [0x50, 0x4b, 0x01, 0x02])
            .map(|(i, _)| i)
            .collect()
    }

    /// Adds `delta` to the little-endian `u32` at `at`.
    fn shift_u32(zip: &mut [u8], at: usize, delta: i64) {
        let value = u32::from_le_bytes(zip[at..at + 4].try_into().unwrap());
        let value = (i64::from(value) + delta) as u32;
        zip[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn text(len: u32) -> Vec<u8> {
        (0..len).map(|i| b'a' + (i % 26) as u8).collect()
    }

    #[test]
    fn checksums_are_checked_at_the_end() {
        let zip = ZipBuilder::new()
            .stored("s.txt", text(5000))
            .deflated("d.txt", text(5000))
            .build();
        let storage = OneFile(zip.clone());
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        for name in ["s.txt", "d.txt"] {
            let entry = archive.entry(&storage, name).unwrap().unwrap();
            assert_eq!(archive.read(&storage, &entry, 1 << 20).unwrap(), text(5000));
        }

        // A changed byte in the stored entry; the checksum of the deflated one.
        let mut damaged = zip;
        damaged[30 + 5 + 100] ^= 1;
        let deflated = records(&damaged)[1];
        damaged[deflated + 16] ^= 1;
        let storage = OneFile(damaged);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        for name in ["s.txt", "d.txt"] {
            let entry = archive.entry(&storage, name).unwrap().unwrap();
            assert_eq!(
                archive.read(&storage, &entry, 1 << 20),
                Err(ZipError::Invalid("checksum mismatch")),
                "{name}"
            );
            // Readers that stop before the end do not check.
            archive.stream(&storage, &entry, &mut |_| false).unwrap();
        }
    }

    #[test]
    fn sizes_are_checked() {
        let zip = ZipBuilder::new()
            .stored("s.txt", text(5000))
            .deflated("d.txt", text(5000))
            .build();
        // The deflated entry claims to be longer, then shorter (so that it
        // would outgrow a window sized by its claim) than it is.
        for delta in [1, -1, -4900] {
            let mut zip = zip.clone();
            let deflated = records(&zip)[1];
            shift_u32(&mut zip, deflated + 24, delta);
            let storage = OneFile(zip);
            let archive = ZipArchive::open(&storage, "x.zip").unwrap();
            let entry = archive.entry(&storage, "d.txt").unwrap().unwrap();
            assert!(
                matches!(
                    archive.read(&storage, &entry, 1 << 20),
                    Err(ZipError::Invalid(_))
                ),
                "{delta}"
            );
        }
        // A stored entry whose two sizes differ.
        let mut zip = zip.clone();
        let stored = records(&zip)[0];
        shift_u32(&mut zip, stored + 24, -10);
        let storage = OneFile(zip);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        assert!(matches!(
            archive.entry(&storage, "s.txt"),
            Err(ZipError::Invalid(_))
        ));
    }

    #[test]
    fn names_differing_in_case_are_found() {
        let zip = ZipBuilder::new()
            .stored("OEBPS/Text/Chapter1.xhtml", "one")
            .stored("a.txt", "small")
            .stored("A.txt", "capital")
            .build();
        let storage = OneFile(zip);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        let found = |name| {
            archive
                .entry(&storage, name)
                .unwrap()
                .map(|entry| entry.name)
        };
        assert_eq!(
            found("oebps/text/chapter1.XHTML").as_deref(),
            Some("OEBPS/Text/Chapter1.xhtml")
        );
        // The exact name first.
        assert_eq!(found("a.txt").as_deref(), Some("a.txt"));
        assert_eq!(found("A.txt").as_deref(), Some("A.txt"));
        assert_eq!(found("b.txt"), None);
    }

    #[test]
    fn names_that_are_not_utf8_are_found_as_listed() {
        let mut zip = ZipBuilder::new().stored("x?.txt", "data").build();
        for at in 0..zip.len() - 5 {
            if &zip[at..at + 6] == b"x?.txt" {
                zip[at + 1] = 0xff;
            }
        }
        let storage = OneFile(zip);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        let listed = archive.entries(&storage).next().unwrap().unwrap();
        assert_eq!(listed.name, "x\u{fffd}.txt");
        let found = archive.entry(&storage, &listed.name).unwrap();
        assert_eq!(found, Some(listed));
    }

    #[test]
    fn records_are_checked() {
        let storage = OneFile(ZipBuilder::new().stored("a.txt", "a").build());
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        // Offset 0 holds a local header, not a central directory record.
        assert!(matches!(
            archive.entry_by_record(&storage, 0),
            Err(ZipError::Invalid(_))
        ));
    }

    #[test]
    fn end_record_inside_the_comment_is_not_taken() {
        let mut zip = ZipBuilder::new().stored("a.txt", "a").build();
        // A comment that holds what looks like an empty archive's record.
        let mut comment = b"see ".to_vec();
        comment.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        comment.extend_from_slice(&[0; 18]);
        comment.extend_from_slice(b" end");
        let len = zip.len();
        zip[len - 2..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        zip.extend_from_slice(&comment);
        let storage = OneFile(zip);
        let archive = ZipArchive::open(&storage, "x.zip").unwrap();
        assert_eq!(archive.len(), 1);
    }

    #[test]
    fn crc32_matches_zip() {
        assert_eq!(crc32(0, b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xcbf4_3926);
        assert_eq!(crc32(0, b""), 0);
        assert_eq!(
            crc32(0, b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
        // As going bit by bit, over every byte value.
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let mut bitwise = !0u32;
        for &byte in &data {
            bitwise ^= u32::from(byte);
            for _ in 0..8 {
                bitwise = if bitwise & 1 == 1 {
                    (bitwise >> 1) ^ 0xedb8_8320
                } else {
                    bitwise >> 1
                };
            }
        }
        assert_eq!(crc32(0, &data), !bitwise);
    }

    #[test]
    fn rejects_non_zip_data() {
        let storage = OneFile(b"this is not a zip archive at all, sorry".to_vec());
        assert!(matches!(
            ZipArchive::open(&storage, "x"),
            Err(ZipError::Invalid(_))
        ));
        let _ = "x".to_owned().to_string();
    }
}
