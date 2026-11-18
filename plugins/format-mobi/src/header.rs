//! The Palm database a Mobipocket book is, its first record (the PalmDOC
//! and MOBI headers) and the EXTH records after them (author, language,
//! cover, ...). Everything is read with bounds: a header that claims more
//! than the file has, or more than a small device may hold, is refused.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{DocumentError, Metadata};

use crate::mem::{self, Refused};
use crate::text::{Encoding, decode_into};

/// Bytes of the first record read: both headers and the EXTH records fit
/// where any book is made by a known tool.
const FIRST_RECORD: usize = 16 * 1024;
/// Records a book may have (a text record is 4 KB: this is ~160 MB).
const MOST_RECORDS: u32 = 40_000;
/// The longest text a book may have, in bytes.
pub(crate) const MOST_TEXT: u32 = 24 * 1024 * 1024;
/// Text record sizes read (the format makes them 4096).
const RECORD_SIZES: core::ops::RangeInclusive<u32> = 256..=16 * 1024;
/// EXTH records read at most.
const MOST_EXTH: u32 = 512;

const COMPRESSION_NONE: u16 = 1;
const COMPRESSION_PALMDOC: u16 = 2;
const COMPRESSION_HUFFMAN: u16 = 17480;

/// What the first record tells.
#[derive(Debug, Clone)]
pub(crate) struct Header {
    /// Records of the database.
    pub records: u32,
    pub text_records: u32,
    pub record_size: u32,
    pub text_length: u32,
    pub compressed: bool,
    pub encoding: Encoding,
    /// Which kinds of data follow each text record's text.
    pub extra_flags: u16,
    /// The first picture's record (0: not said).
    pub first_image: u32,
    /// The record of the cover picture, where the book says.
    pub cover: Option<u32>,
    pub metadata: Metadata,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn malformed(why: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(why))
}

/// Where record `index` starts in the file, from the database's list of
/// records (8 bytes each after its 78-byte header).
pub(crate) fn record_start(
    storage: &dyn Storage,
    path: &str,
    index: u32,
) -> Result<u32, DocumentError> {
    let mut entry = [0u8; 4];
    let n = storage.read_at(path, 78 + 8 * u64::from(index), &mut entry)?;
    if n < 4 {
        return Err(malformed("the list of records is cut off"));
    }
    Ok(u32::from_be_bytes(entry))
}

/// Where record `index` is in the file, and how long: up to the next
/// record's start (or the file's end), never outside the file.
pub(crate) fn record_range(
    storage: &dyn Storage,
    path: &str,
    records: u32,
    size: u64,
    index: u32,
) -> Result<Option<(u64, u64)>, DocumentError> {
    if index >= records {
        return Ok(None);
    }
    // A list of records that stops short is no record; a card that fails is
    // an error.
    let at = |index| match record_start(storage, path, index) {
        Ok(start) => Ok(Some(u64::from(start))),
        Err(DocumentError::Malformed(_)) => Ok(None),
        Err(error) => Err(error),
    };
    let Some(start) = at(index)? else {
        return Ok(None);
    };
    let end = if index + 1 < records {
        let Some(end) = at(index + 1)? else {
            return Ok(None);
        };
        end
    } else {
        size
    };
    Ok((start <= end && end <= size).then_some((start, end)))
}

fn text(bytes: &[u8], encoding: Encoding) -> Result<String, Refused> {
    let mut out = String::new();
    decode_into(bytes, encoding, &mut out)?;
    mem::string(out.trim())
}

/// The language tag of a Windows locale's low byte, for the common ones.
fn locale_language(locale: u32) -> Option<&'static str> {
    Some(match locale & 0xff {
        9 => "en",
        7 => "de",
        12 => "fr",
        10 => "es",
        16 => "it",
        19 => "nl",
        22 => "pt",
        25 => "ru",
        29 => "sv",
        21 => "pl",
        14 => "hu",
        5 => "cs",
        8 => "el",
        4 => "zh",
        17 => "ja",
        18 => "ko",
        _ => return None,
    })
}

impl Header {
    /// Reads the headers of the book at `path`.
    pub(crate) fn read(storage: &dyn Storage, path: &str) -> Result<Header, DocumentError> {
        let size = storage.size(path)?;
        let mut pdb = [0u8; 78];
        let n = storage.read_at(path, 0, &mut pdb)?;
        if n < pdb.len() || !(&pdb[60..68] == b"BOOKMOBI" || &pdb[60..68] == b"TEXtREAd") {
            return Err(malformed("this is not a Mobipocket book"));
        }
        let records = u32::from(be16(&pdb, 76).unwrap_or(0));
        if !(2..=MOST_RECORDS).contains(&records) || u64::from(records) * 8 + 78 > size {
            return Err(malformed("the list of records is wrong"));
        }
        let (start, end) = record_range(storage, path, records, size, 0)?
            .ok_or_else(|| malformed("the first record is wrong"))?;
        let want = (end - start).min(FIRST_RECORD as u64) as usize;
        let mut first: Vec<u8> = Vec::new();
        first.try_reserve_exact(want).map_err(|_| Refused)?;
        first.resize(want, 0);
        let n = storage.read_at(path, start, &mut first)?;
        first.truncate(n);
        Self::parse(&first, records, storage, path, size)
    }

    fn parse(
        first: &[u8],
        records: u32,
        storage: &dyn Storage,
        path: &str,
        size: u64,
    ) -> Result<Header, DocumentError> {
        let too_short = || malformed("the first record is cut off");
        let compression = be16(first, 0).ok_or_else(too_short)?;
        let text_length = be32(first, 4).ok_or_else(too_short)?;
        let text_records = u32::from(be16(first, 8).ok_or_else(too_short)?);
        let record_size = u32::from(be16(first, 10).ok_or_else(too_short)?);
        let encryption = be16(first, 12).ok_or_else(too_short)?;
        if encryption != 0 {
            return Err(malformed(
                "this book is protected by DRM and cannot be opened",
            ));
        }
        match compression {
            COMPRESSION_NONE | COMPRESSION_PALMDOC => {}
            COMPRESSION_HUFFMAN => {
                return Err(malformed(
                    "this book uses Huffman compression (HUFF/CDIC), which PaperPaw cannot read yet",
                ));
            }
            _ => return Err(malformed("this book's compression is not known")),
        }
        if !RECORD_SIZES.contains(&record_size) {
            return Err(malformed("the text records have a wrong size"));
        }
        if text_records == 0 || text_records >= records || text_length > MOST_TEXT {
            return Err(malformed("the text is wrong or too long"));
        }
        // Text records hold `record_size` bytes each but the last.
        let text_length = text_length.min(text_records * record_size);
        let mut header = Header {
            records,
            text_records,
            record_size,
            text_length,
            compressed: compression == COMPRESSION_PALMDOC,
            encoding: Encoding::Windows1252,
            extra_flags: 0,
            first_image: 0,
            cover: None,
            metadata: Metadata::default(),
        };
        if first.get(16..20) != Some(b"MOBI") {
            // A plain PalmDOC book: text and nothing else.
            return Ok(header);
        }
        let length = be32(first, 20).ok_or_else(too_short)? as usize;
        let version = be32(first, 36).unwrap_or(0);
        if version >= 8 {
            return Err(malformed(
                "this is a KF8 (AZW3) book, which PaperPaw cannot read yet",
            ));
        }
        header.encoding = if be32(first, 28) == Some(65001) {
            Encoding::Utf8
        } else {
            Encoding::Windows1252
        };
        let locale = be32(first, 92).unwrap_or(0);
        header.metadata.language = match locale_language(locale) {
            Some(language) => Some(mem::string(language)?),
            None => None,
        };
        header.first_image = be32(first, 108).filter(|&i| i < records).unwrap_or(0);
        if length >= 0xe4 {
            header.extra_flags = be16(first, 0xf2).unwrap_or(0);
        }
        let name = be32(first, 84).zip(be32(first, 88));
        if let Some((at, len)) = name
            && let Some(bytes) = (at as usize)
                .checked_add(len as usize)
                .and_then(|end| first.get(at as usize..end))
            && !bytes.is_empty()
        {
            header.metadata.title = Some(text(bytes, header.encoding)?).filter(|t| !t.is_empty());
        }
        let has_exth = be32(first, 128).is_some_and(|flags| flags & 0x40 != 0);
        if has_exth {
            header.exth(first, 16 + length)?;
        }
        // A text record that points past the file is not a book.
        if record_range(storage, path, records, size, text_records)?.is_none() {
            return Err(malformed("the text is cut off"));
        }
        Ok(header)
    }

    /// The EXTH records at `at`: what the book says of itself.
    fn exth(&mut self, data: &[u8], at: usize) -> Result<(), Refused> {
        if data.get(at..at + 4) != Some(b"EXTH") {
            return Ok(());
        }
        let count = be32(data, at + 8).unwrap_or(0).min(MOST_EXTH);
        let mut pos = at + 12;
        let mut authors: Vec<String> = Vec::new();
        let mut cover = None;
        for _ in 0..count {
            let (Some(kind), Some(len)) = (be32(data, pos), be32(data, pos + 4)) else {
                break;
            };
            let len = len as usize;
            let Some(body) = (len >= 8).then(|| data.get(pos + 8..pos + len)).flatten() else {
                break;
            };
            pos += len;
            let encoding = self.encoding;
            let string = || -> Result<Option<String>, Refused> {
                Ok(Some(text(body, encoding)?).filter(|s| !s.is_empty()))
            };
            match kind {
                100 => {
                    if let Some(author) = string()? {
                        mem::push(&mut authors, author)?;
                    }
                }
                103 => self.metadata.description = string()?,
                105 => {
                    if let Some(subject) = string()? {
                        mem::push(&mut self.metadata.subjects, subject)?;
                    }
                }
                201 => cover = be32(body, 0),
                503 => self.metadata.title = string()?.or(self.metadata.title.take()),
                524 => self.metadata.language = string()?.or(self.metadata.language.take()),
                _ => {}
            }
        }
        self.metadata.authors = authors;
        self.cover = cover
            .and_then(|offset| self.first_image.checked_add(offset))
            .filter(|&record| self.first_image > 0 && record < self.records);
        Ok(())
    }
}
