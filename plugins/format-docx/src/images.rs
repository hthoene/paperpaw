//! The pictures of a document: taken out of the archive into the document's
//! folder when their page shows, then decoded a few rows at a time into
//! gray (or colour) files. Every allocation can be refused: that is
//! `OutOfMemory`, never a picture named as damaged; so is a failing card.

use alloc::string::String;

use paperpaw_sdk::document::{DocumentError, ImageRef};
use paperpaw_sdk::image::{
    ColorWriter, Fit, Format, GrayWriter, ImageError, Source, decode, dimensions,
};
use paperpaw_sdk::zip::{ZipArchive, ZipError};
use paperpaw_sdk::{Storage, StorageError};

use crate::mem::{self, Refused};
use crate::{MAX_IMAGE, MAX_PATH, cache};

/// Bytes of a picture read to tell its size.
const HEAD: usize = 4096;
/// The list of the pictures' places in the archive, one a line.
pub(crate) const LIST: &str = "pictures.txt";

fn oom(_: Refused) -> DocumentError {
    DocumentError::OutOfMemory
}

/// The file `args` names in the folder `dir`.
fn named(dir: &str, args: core::fmt::Arguments<'_>) -> Result<String, Refused> {
    let name = mem::formatted(args)?;
    cache::path(dir, &name)
}

/// The file picture `index` is taken out into.
fn media(dir: &str, index: u32) -> Result<String, Refused> {
    named(dir, format_args!("media-{index}"))
}

/// The file picture `index` is decoded into.
pub(crate) fn file(dir: &str, index: u32, color: bool) -> Result<String, Refused> {
    named(
        dir,
        format_args!("img-{index}.{}", if color { "rgb" } else { "gray" }),
    )
}

/// The file picture `index` is decoded into at `side` pixels, larger than on
/// its page (a zoom).
fn zoomed(dir: &str, index: u32, side: u32, color: bool) -> Result<String, Refused> {
    named(
        dir,
        format_args!("zoom-{index}-{side}.{}", if color { "rgb" } else { "gray" }),
    )
}

/// The marker of a picture that cannot be shown.
fn failed(dir: &str, index: u32) -> Result<String, Refused> {
    named(dir, format_args!("img-{index}.fail"))
}

/// The picture number in the name of a file made by [`file`] or [`zoomed`].
pub(crate) fn index_of(path: &str) -> Option<u32> {
    let name = path.rsplit('/').next()?;
    let (stem, _) = name.rsplit_once('.')?;
    let number = stem
        .strip_prefix("img-")
        .or_else(|| stem.strip_prefix("zoom-"))?;
    number.split('-').next()?.parse().ok()
}

/// Where picture `index` is in the archive: line `index` of the list, read
/// a piece at a time (a line longer than a path is none).
fn place(storage: &dyn Storage, dir: &str, index: u32) -> Result<Option<String>, DocumentError> {
    let list = cache::path(dir, LIST).map_err(oom)?;
    let mut chunk = [0u8; 256];
    let (mut at, mut line) = (0u64, 0u32);
    let mut found: Option<alloc::vec::Vec<u8>> = None;
    loop {
        let n = match storage.read_at(&list, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if n == 0 {
            break;
        }
        at += n as u64;
        for &byte in &chunk[..n] {
            if byte == b'\n' {
                if line == index {
                    return text(found);
                }
                line += 1;
            } else if line == index {
                let bytes = found.get_or_insert_with(alloc::vec::Vec::new);
                if bytes.len() >= MAX_PATH {
                    return Ok(None);
                }
                mem::push(bytes, byte).map_err(oom)?;
            }
        }
    }
    // The last line has no newline after it.
    if line == index { text(found) } else { Ok(None) }
}

fn text(bytes: Option<alloc::vec::Vec<u8>>) -> Result<Option<String>, DocumentError> {
    Ok(bytes.and_then(|bytes| String::from_utf8(bytes).ok()))
}

/// A whole file as a source of image bytes.
struct Whole<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    offset: u64,
}

impl Source for Whole<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
        let n = self
            .storage
            .read_at(self.path, self.offset, buf)
            .map_err(|_| ImageError::Io)?;
        self.offset += n as u64;
        Ok(n)
    }
}

/// The picture decoded into `path`, if it is: the header the writers made
/// (`PPGI` gray or `PPCI` colour, width and height) and a file of exactly
/// the size it names. Read here, as `ImageFile::open` makes its path string
/// without a way to refuse (a named step in `PORTING-7.md`).
fn decoded(
    storage: &dyn Storage,
    path: &str,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    let mut header = [0u8; 12];
    match storage.read_at(path, 0, &mut header) {
        Ok(12) => {}
        Ok(_) | Err(StorageError::NotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let (magic, bytes): (&[u8; 4], u64) = if color { (b"PPCI", 3) } else { (b"PPGI", 1) };
    let field = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let (width, height) = (field(4), field(8));
    if &header[..4] != magic || width == 0 || height == 0 {
        return Ok(None);
    }
    let expected = (u64::from(width) * u64::from(height))
        .checked_mul(bytes)
        .and_then(|n| n.checked_add(12));
    match (storage.size(path), expected) {
        (Ok(size), Some(expected)) if size == expected => Ok(Some(ImageRef {
            path: mem::string(path).map_err(oom)?,
            width,
            height,
        })),
        (Err(error), _) if error != StorageError::NotFound => Err(error.into()),
        _ => Ok(None),
    }
}

/// Whether the file `path` holds a decoded picture.
pub(crate) fn is_decoded(
    storage: &dyn Storage,
    path: &str,
    color: bool,
) -> Result<bool, DocumentError> {
    Ok(decoded(storage, path, color)?.is_some())
}

/// Takes picture `index` out of the archive `path`, unless it is out: its
/// file, or `None` where the archive has none that is usable.
fn take_out(
    storage: &dyn Storage,
    path: &str,
    dir: &str,
    index: u32,
) -> Result<Option<String>, DocumentError> {
    let to = media(dir, index).map_err(oom)?;
    match storage.size(&to) {
        Ok(size) if size > 0 => return Ok(Some(to)),
        Ok(_) | Err(StorageError::NotFound) => {}
        Err(error) => return Err(error.into()),
    }
    let Some(from) = place(storage, dir, index)? else {
        return Ok(None);
    };
    let archive = match ZipArchive::open(storage, path) {
        Ok(archive) => archive,
        Err(error) => return damaged(error),
    };
    let entry = match archive.entry(storage, &from) {
        Ok(Some(entry)) => entry,
        Ok(None) => return Ok(None),
        Err(error) => return damaged(error),
    };
    if entry.size > MAX_IMAGE {
        return Ok(None);
    }
    storage.write(&to, &[])?;
    let mut failed = None;
    let streamed = archive.stream(
        storage,
        &entry,
        &mut |chunk| match storage.append(&to, chunk) {
            Ok(()) => true,
            Err(error) => {
                failed = Some(error);
                false
            }
        },
    );
    let result = match (failed, streamed) {
        (Some(error), _) => Err(error.into()),
        (None, Ok(())) => return Ok(Some(to)),
        (None, Err(error)) => damaged(error),
    };
    storage.remove(&to).ok();
    result
}

/// What a zip error says of a picture: memory or the card failing is the
/// caller's, anything else is a picture the archive does not hold.
fn damaged<T>(error: ZipError) -> Result<Option<T>, DocumentError> {
    match error {
        ZipError::OutOfMemory => Err(DocumentError::OutOfMemory),
        ZipError::Storage(error) => Err(error.into()),
        _ => Ok(None),
    }
}

/// The picture `index` as it will be shown: its size, no larger than
/// `fit`, in the file it is decoded into (which [`prepare`] makes).
pub(crate) fn picture(
    storage: &dyn Storage,
    path: &str,
    dir: &str,
    index: u32,
    fit: Fit,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    let to = file(dir, index, color).map_err(oom)?;
    if let Some(done) = decoded(storage, &to, color)? {
        return Ok(Some(done));
    }
    let marker = failed(dir, index).map_err(oom)?;
    if storage.exists(&marker) {
        return Ok(None);
    }
    let mut head = [0u8; HEAD];
    let shown = match take_out(storage, path, dir, index)? {
        None => None,
        Some(from) => {
            let n = storage.read_at(&from, 0, &mut head)?;
            Format::detect(&head[..n]).and_then(|_| {
                let (width, height) = dimensions(&head[..n])?;
                let (width, height) = fit.size(width, height);
                (width > 0 && height > 0).then_some((width, height))
            })
        }
    };
    let Some((width, height)) = shown else {
        storage.write(&marker, b"").ok();
        return Ok(None);
    };
    Ok(Some(ImageRef {
        path: to,
        width,
        height,
    }))
}

/// Decodes the picture `index` into `to` (no larger than `fit`): `None`
/// where it cannot be (it is marked then).
fn decode_into(
    storage: &dyn Storage,
    dir: &str,
    index: u32,
    to: &str,
    fit: Fit,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    let from = media(dir, index).map_err(oom)?;
    let marker = failed(dir, index).map_err(oom)?;
    let mut head = [0u8; 8];
    let n = storage.read_at(&from, 0, &mut head)?;
    let Some(format) = Format::detect(&head[..n]) else {
        storage.write(&marker, b"").ok();
        return Ok(None);
    };
    let mut source = Whole {
        storage,
        path: &from,
        offset: 0,
    };
    let result = if color {
        let mut writer = ColorWriter::new(storage, to);
        decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
    } else {
        let mut writer = GrayWriter::new(storage, to);
        decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())
    };
    if let Err(error) = result {
        storage.remove(to).ok();
        return match error {
            ImageError::OutOfMemory => Err(DocumentError::OutOfMemory),
            ImageError::Io => Err(StorageError::Io.into()),
            ImageError::Storage(error) => Err(error.into()),
            _ => {
                storage.write(&marker, b"").ok();
                Ok(None)
            }
        };
    }
    match decoded(storage, to, color)? {
        Some(done) => Ok(Some(done)),
        None => {
            storage.remove(to).ok();
            storage.write(&marker, b"").ok();
            Ok(None)
        }
    }
}

/// Decodes the picture of `image` into its file; `None` where it is there
/// already or cannot be.
pub(crate) fn prepare(
    storage: &dyn Storage,
    dir: &str,
    image: &ImageRef,
    fit: Fit,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    if is_decoded(storage, &image.path, color)? {
        return Ok(None);
    }
    let Some(index) = index_of(&image.path) else {
        return Ok(None);
    };
    decode_into(storage, dir, index, &image.path, fit, color)
}

/// The picture `index` decoded: at the page's size (`fit`), or at `side`
/// pixels where that is more (a zoom). `None` where there is no usable
/// picture.
pub(crate) fn shown_at(
    storage: &dyn Storage,
    path: &str,
    dir: &str,
    index: u32,
    side: u32,
    fit: Fit,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    let (to, fit) = if side > fit.long {
        (
            zoomed(dir, index, side, color).map_err(oom)?,
            Fit::side(side),
        )
    } else {
        (file(dir, index, color).map_err(oom)?, fit)
    };
    if let Some(done) = decoded(storage, &to, color)? {
        return Ok(Some(done));
    }
    let marker = failed(dir, index).map_err(oom)?;
    if storage.exists(&marker) {
        return Ok(None);
    }
    if take_out(storage, path, dir, index)?.is_none() {
        storage.write(&marker, b"").ok();
        return Ok(None);
    }
    decode_into(storage, dir, index, &to, fit, color)
}
