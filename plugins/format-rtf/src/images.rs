//! The pictures of a document: the JPEG and PNG pictures in its text, whose
//! hexadecimal data is read where it is in the file and decoded a few rows
//! at a time into gray (or colour) files of the cache folder, when their
//! page shows.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{DocumentError, ImageRef};
use paperpaw_sdk::image::{
    ColorWriter, Fit, Format, GrayWriter, ImageError, ImageFile, Source, decode, dimensions,
};
use paperpaw_sdk::{Storage, StorageError};

use crate::mem::{self, Refused};
use crate::rtf::Pictures;

/// Bytes of a picture read to tell its size.
const HEAD: usize = 4096;
/// The hexadecimal text at a part of a file as a source of image bytes: two
/// digits a byte, anything else (line ends, spaces) between them left out.
struct HexRange<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    offset: u64,
    end: u64,
    /// The first digit of a byte whose second is in the next read.
    high: Option<u8>,
}

fn digit(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

impl Source for HexRange<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
        let mut raw = [0u8; 256];
        let mut done = 0;
        while done < buf.len() && self.offset < self.end {
            let want = (self.end - self.offset)
                .min(raw.len() as u64)
                .min(2 * (buf.len() - done) as u64) as usize;
            let n = self
                .storage
                .read_at(self.path, self.offset, &mut raw[..want])
                .map_err(|_| ImageError::Io)?;
            if n == 0 {
                self.offset = self.end;
                break;
            }
            self.offset += n as u64;
            for &byte in &raw[..n] {
                let Some(value) = digit(byte) else {
                    continue;
                };
                match self.high.take() {
                    None => self.high = Some(value),
                    Some(high) => {
                        buf[done] = high << 4 | value;
                        done += 1;
                    }
                }
            }
        }
        Ok(done)
    }
}

/// Where the pictures are: the document.
pub(crate) struct Book<'a> {
    pub storage: &'a dyn Storage,
    pub path: &'a str,
    pub fit: Fit,
}

impl Book<'_> {
    /// Decodes the picture whose hexadecimal data is at `range` into the
    /// file `to`, no larger than `fit`: `None` for a picture that is none
    /// (damaged, or of a kind that is not decoded); a card or memory that
    /// fails is an error.
    pub(crate) fn decode(
        &self,
        range: (u32, u32),
        to: &str,
        fit: Fit,
        color: bool,
    ) -> Result<Option<ImageRef>, DocumentError> {
        let mut source = HexRange {
            storage: self.storage,
            path: self.path,
            offset: u64::from(range.0),
            end: u64::from(range.1),
            high: None,
        };
        let mut head = [0u8; 8];
        let n = source.read(&mut head).map_err(|_| StorageError::Io)?;
        let Some(format) = Format::detect(&head[..n]) else {
            return Ok(None);
        };
        // Again from the first digit.
        source.offset = u64::from(range.0);
        source.high = None;
        let result = if color {
            let mut writer = ColorWriter::new(self.storage, to);
            decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
        } else {
            let mut writer = GrayWriter::new(self.storage, to);
            decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
        };
        if let Err(error) = result {
            self.storage.remove(to).ok();
            return match error {
                ImageError::Io => Err(StorageError::Io.into()),
                ImageError::Storage(error) => Err(error.into()),
                ImageError::OutOfMemory => Err(DocumentError::OutOfMemory),
                _ => Ok(None),
            };
        }
        Ok(ImageFile::open(self.storage, to)
            .filter(|file| file.width > 0 && file.height > 0)
            .map(|file| ImageRef {
                path: file.path,
                width: file.width,
                height: file.height,
            }))
    }
}

impl Pictures for Book<'_> {
    fn size(&mut self, start: u32, end: u32) -> Result<Option<(u32, u32)>, DocumentError> {
        // On the heap: 4 KiB on the stack is a frame the X4 refuses (2 KiB).
        let mut head: Vec<u8> = Vec::new();
        head.try_reserve_exact(HEAD)
            .map_err(|_| DocumentError::OutOfMemory)?;
        head.resize(HEAD, 0);
        let mut source = HexRange {
            storage: self.storage,
            path: self.path,
            offset: u64::from(start),
            end: u64::from(end),
            high: None,
        };
        let mut n = 0;
        while n < HEAD {
            let got = source.read(&mut head[n..]).map_err(|error| match error {
                ImageError::Io => DocumentError::Storage(StorageError::Io),
                _ => DocumentError::OutOfMemory,
            })?;
            if got == 0 {
                break;
            }
            n += got;
        }
        if Format::detect(&head[..n]).is_none() {
            return Ok(None);
        }
        let Some((width, height)) = dimensions(&head[..n]) else {
            return Ok(None);
        };
        let (width, height) = self.fit.size(width, height);
        Ok((width > 0 && height > 0).then_some((width, height)))
    }
}

/// `dir/name`, or `name` where the folder is the cache itself.
fn named(dir: &str, args: core::fmt::Arguments<'_>) -> Result<String, Refused> {
    if dir.is_empty() {
        mem::formatted(args)
    } else {
        mem::formatted(format_args!("{dir}/{args}"))
    }
}

/// The file the picture of record number `index` is decoded into.
pub(crate) fn file(dir: &str, index: u32, color: bool) -> Result<String, Refused> {
    named(
        dir,
        format_args!("img-{index}.{}", if color { "rgb" } else { "gray" }),
    )
}

/// The file the picture of record number `index` is decoded into at `side`
/// pixels, larger than on its page (a zoom).
pub(crate) fn zoomed(dir: &str, index: u32, side: u32, color: bool) -> Result<String, Refused> {
    named(
        dir,
        format_args!("zoom-{index}-{side}.{}", if color { "rgb" } else { "gray" }),
    )
}

/// The marker of a picture that could not be decoded.
pub(crate) fn failed(dir: &str, index: u32) -> Result<String, Refused> {
    named(dir, format_args!("img-{index}.fail"))
}

/// The picture number in the name of a file made by [`file`].
pub(crate) fn index_of(path: &str) -> Option<u32> {
    let name = path.rsplit('/').next()?;
    let (stem, _) = name.strip_prefix("img-")?.rsplit_once('.')?;
    stem.parse().ok()
}
