//! Reading pictures for a chapter: the size of a picture from its head,
//! and its decoding from the archive or from a file in storage.

use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::zip::{Entry, ZipArchive};

use super::*;

/// Reads part of a file in storage.
struct StorageSource<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    offset: u64,
    end: u64,
}

impl paperpaw_sdk::image::Source for StorageSource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, paperpaw_sdk::image::ImageError> {
        let left = (self.end - self.offset).min(buf.len() as u64) as usize;
        if left == 0 {
            return Ok(0);
        }
        let n = self
            .storage
            .read_at(self.path, self.offset, &mut buf[..left])
            .map_err(|_| paperpaw_sdk::image::ImageError::Io)?;
        self.offset += n as u64;
        Ok(n)
    }
}

/// Bytes read from a picture to tell its size.
const SIZE_HEAD: usize = 16 * 1024;

/// The size of picture `path` of the archive, from its first bytes.
pub(super) fn image_size(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
) -> Option<(u32, u32)> {
    let entry = archive.entry(storage, path).ok()??;
    let mut head = Vec::new();
    head.try_reserve_exact(SIZE_HEAD).ok()?;
    head.resize(SIZE_HEAD, 0);
    let mut filled = 0;
    match archive.stored_range(storage, &entry).ok()? {
        Some((offset, len)) => {
            let want = (len as usize).min(SIZE_HEAD);
            while filled < want {
                let n = storage
                    .read_at(
                        archive.path(),
                        offset + filled as u64,
                        &mut head[filled..want],
                    )
                    .ok()?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
        }
        None => {
            let mut reader = archive.reader(storage, &entry).ok()?;
            while filled < SIZE_HEAD {
                let n = reader.read(storage, &mut head[filled..]).ok()?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
        }
    }
    paperpaw_sdk::image::dimensions(&head[..filled])
}

/// A compressed entry of the archive, inflated as it is read.
struct EntrySource<'a> {
    storage: &'a dyn Storage,
    reader: paperpaw_sdk::zip::EntryReader<'a>,
}

impl paperpaw_sdk::image::Source for EntrySource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, paperpaw_sdk::image::ImageError> {
        self.reader
            .read(self.storage, buf)
            .map_err(|_| paperpaw_sdk::image::ImageError::Io)
    }
}

/// A source whose first bytes were read to tell its format, given again
/// first.
struct Headed<'a> {
    head: [u8; 8],
    len: usize,
    at: usize,
    rest: &'a mut dyn paperpaw_sdk::image::Source,
}

impl paperpaw_sdk::image::Source for Headed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, paperpaw_sdk::image::ImageError> {
        if self.at < self.len {
            let n = (self.len - self.at).min(buf.len());
            buf[..n].copy_from_slice(&self.head[self.at..self.at + n]);
            self.at += n;
            return Ok(n);
        }
        self.rest.read(buf)
    }
}

/// Decodes the image at `path` in the archive into the cache file
/// `cache`, in colour with `color`, else gray. Stored entries are read in
/// place, compressed ones inflated as they are decoded (or, with memory
/// short for both at once, inflated to a temporary file first).
pub(crate) fn decode_image(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
    cache: &str,
    dir: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    let entry = archive
        .entry(storage, path)
        .map_err(error)?
        .ok_or(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "missing image",
        )))?;
    decode_entry(storage, archive, &entry, cache, dir, fit, color)
}

/// Decodes the image of archive entry `entry` as [`decode_image`] does.
pub(crate) fn decode_entry(
    storage: &dyn Storage,
    archive: &ZipArchive,
    entry: &Entry,
    cache: &str,
    dir: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    if let Some((offset, len)) = archive.stored_range(storage, entry).map_err(error)? {
        return decode_range(
            storage,
            archive.path(),
            offset,
            offset + len,
            cache,
            fit,
            color,
        );
    }
    let streamed = match archive.reader(storage, entry) {
        Ok(reader) => {
            let mut source = EntrySource { storage, reader };
            decode_source(storage, &mut source, cache, fit, color)
        }
        Err(paperpaw_sdk::zip::ZipError::OutOfMemory) => {
            Err(paperpaw_sdk::image::ImageError::OutOfMemory)
        }
        Err(e) => return Err(error(e)),
    };
    if streamed != Err(paperpaw_sdk::image::ImageError::OutOfMemory) {
        return streamed.map_err(image_error);
    }
    let temp = alloc::format!("{dir}/image.tmp");
    storage.write(&temp, &[])?;
    let mut failed = None;
    archive
        .stream(storage, entry, &mut |chunk| {
            failed = storage.append(&temp, chunk).err();
            failed.is_none()
        })
        .map_err(error)?;
    let result = match failed {
        Some(e) => Err(DocumentError::Storage(e)),
        None => decode_range(storage, &temp, 0, u64::from(entry.size), cache, fit, color),
    };
    let _ = storage.remove(&temp);
    result
}

/// Decodes the image file `file` into the cache file `cache`, in colour
/// with `color`.
pub(crate) fn decode_file(
    storage: &dyn Storage,
    file: &str,
    cache: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    let end = storage.size(file)?;
    decode_range(storage, file, 0, end, cache, fit, color)
}

/// Decodes the image in `offset..end` of `file` into `cache`.
pub(super) fn decode_range(
    storage: &dyn Storage,
    file: &str,
    offset: u64,
    end: u64,
    cache: &str,
    fit: Fit,
    color: bool,
) -> Result<(), DocumentError> {
    let mut source = StorageSource {
        storage,
        path: file,
        offset,
        end,
    };
    decode_source(storage, &mut source, cache, fit, color).map_err(image_error)
}

/// Decodes the image `source` gives into `cache`; nothing is left of
/// `cache` if it fails.
fn decode_source(
    storage: &dyn Storage,
    source: &mut dyn paperpaw_sdk::image::Source,
    cache: &str,
    fit: Fit,
    color: bool,
) -> Result<(), paperpaw_sdk::image::ImageError> {
    use paperpaw_sdk::image::{ColorWriter, Format, GrayWriter, ImageError, decode};

    let mut head = [0u8; 8];
    let mut len = 0;
    while len < head.len() {
        match source.read(&mut head[len..])? {
            0 => break,
            n => len += n,
        }
    }
    let result = match Format::detect(&head[..len]) {
        Some(format) => {
            let mut source = Headed {
                head,
                len,
                at: 0,
                rest: source,
            };
            if color {
                let mut writer = ColorWriter::new(storage, cache);
                decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
            } else {
                let mut writer = GrayWriter::new(storage, cache);
                decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
            }
        }
        None => Err(ImageError::Unsupported("image format")),
    };
    if result.is_err() {
        let _ = storage.remove(cache);
    }
    result
}

pub(super) fn image_error(e: paperpaw_sdk::image::ImageError) -> DocumentError {
    use paperpaw_sdk::image::ImageError;
    match e {
        ImageError::Io => DocumentError::Storage(paperpaw_sdk::StorageError::Io),
        ImageError::OutOfMemory => DocumentError::OutOfMemory,
        e => DocumentError::Malformed(alloc::borrow::Cow::Owned(alloc::format!("{e}"))),
    }
}
