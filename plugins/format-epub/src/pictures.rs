//! Decoded pictures in the caches: gray (`.gray`) for e-ink, colour
//! (`.rgb`) for colour screens (ADR 0015). Chapters keep a picture's
//! cache name without its kind; it is decoded in the kind the book was
//! opened with, and read in that kind, else in the other one there is.
//!
//! A book's pictures are decoded when a page shows them
//! (`Document::prepare_image`), not when their chapter is converted: then
//! they are noted ([`try_note_all`]) in `images.txt` of the book's cache, a line
//! each with the cache name, the size they are decoded to and their path
//! in the archive, for pages to be laid out meanwhile.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{DocumentError, ImageRef};
use paperpaw_sdk::image::ImageFile;

/// The cache file of picture `base` in colour or gray.
pub(crate) fn file(base: &str, color: bool) -> String {
    format!("{base}.{}", if color { "rgb" } else { "gray" })
}

/// [`file`] where a copy that finds no room is `OutOfMemory`.
pub(crate) fn try_file(base: &str, color: bool) -> Result<String, DocumentError> {
    crate::fallible::try_format!("{base}.{}", if color { "rgb" } else { "gray" })
}

/// The decoded picture `base`, the kind wanted first, else the other; a
/// picture of no size is none. No room is `OutOfMemory`.
pub(crate) fn try_open(
    storage: &dyn Storage,
    base: &str,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    for color in [color, !color] {
        let found = ImageFile::open(storage, &try_file(base, color)?)
            .filter(|image| image.width > 0 && image.height > 0);
        if let Some(image) = found {
            return Ok(Some(ImageRef {
                path: image.path,
                width: image.width,
                height: image.height,
            }));
        }
    }
    Ok(None)
}

/// As [`try_open`], where no room finds no picture (for a lookup that has
/// no way to say so).
pub(crate) fn open(storage: &dyn Storage, base: &str, color: bool) -> Option<ImageRef> {
    try_open(storage, base, color).ok().flatten()
}

/// The mark of picture `base` that could not be decoded.
fn mark(base: &str) -> Option<String> {
    crate::fallible::try_format!("{base}.bad").ok()
}

/// Whether picture `base` was found not to be decodable: it is not tried
/// again each time its page shows (decoding a large one takes seconds on
/// small devices), until its book's cache goes.
pub(crate) fn failed(storage: &dyn Storage, base: &str) -> bool {
    mark(base).is_some_and(|mark| storage.exists(&mark))
}

/// Notes that picture `base` cannot be decoded, where `error` says the
/// picture is at fault: short memory or a storage error may pass.
pub(crate) fn note_failure(storage: &dyn Storage, base: &str, error: &DocumentError) {
    if matches!(error, DocumentError::Malformed(_))
        && let Some(mark) = mark(base)
    {
        let _ = storage.write(&mark, &[]);
    }
}

/// The list of pictures noted in cache folder `dir`.
fn list(dir: &str) -> Result<String, DocumentError> {
    crate::fallible::try_format!("{dir}/images.txt")
}

/// A picture to be noted: its cache name (see [`file`]), the size it is
/// decoded to and its path in the archive.
#[derive(Debug)]
pub(crate) struct Note {
    pub base: String,
    pub size: (u32, u32),
    pub source: String,
}

/// Notes pictures to be decoded when a page shows them, those of one cache
/// folder each, and empties `notes`; a copy that finds no room is
/// `OutOfMemory`. The list is read once for all of them, to leave out those
/// noted before.
pub(crate) fn try_note_all(
    storage: &dyn Storage,
    notes: &mut Vec<Note>,
) -> Result<(), DocumentError> {
    use crate::fallible;
    let Some(dir) = notes
        .first()
        .and_then(|n| n.base.rsplit_once('/'))
        .map(|(d, _)| d)
    else {
        return Ok(());
    };
    let path = fallible::try_format!("{dir}/images.txt")?;
    let mut fresh: Vec<bool> = Vec::new();
    fallible::reserve(&mut fresh, notes.len())?;
    // A picture of no size is never noted: it could not be laid out.
    fresh.extend(notes.iter().map(|note| note.size.0 > 0 && note.size.1 > 0));
    fn name(note: &Note) -> &str {
        note.base.rsplit_once('/').map_or("", |(_, name)| name)
    }
    crate::cached::lines(storage, &path, |line| {
        let noted = line.split('\t').next().unwrap_or_default();
        for (note, fresh) in notes.iter().zip(fresh.iter_mut()) {
            *fresh &= name(note) != noted;
        }
        fresh.contains(&true)
    })?;
    let mut lines = String::new();
    for (note, _) in notes.iter().zip(&fresh).filter(|(_, f)| **f) {
        let (width, height) = note.size;
        fallible::try_write!(
            &mut lines,
            "{}\t{width}\t{height}\t{}\n",
            name(note),
            note.source
        )?;
    }
    let _ = storage.append(&path, lines.as_bytes());
    notes.clear();
    Ok(())
}

/// A noted picture: the size it is decoded to and its path in the archive.
pub(crate) type Noted = ((u32, u32), String);

/// Picture `base` as noted ([`try_note_all`]): the size it is decoded to and
/// its path in the archive. The list is read a little at a time. No room
/// is `OutOfMemory`.
pub(crate) fn try_noted(storage: &dyn Storage, base: &str) -> Result<Option<Noted>, DocumentError> {
    let Some((dir, name)) = base.rsplit_once('/') else {
        return Ok(None);
    };
    let mut found = None;
    let mut refused = None;
    crate::cached::lines(storage, &list(dir)?, |line| {
        match parse(line, name) {
            Ok(note) => found = note,
            Err(error) => refused = Some(error),
        }
        found.is_none() && refused.is_none()
    })?;
    refused.map_or(Ok(found), Err)
}

/// As [`try_noted`], where no room finds no note (for a lookup that has
/// no way to say so).
pub(crate) fn noted(storage: &dyn Storage, base: &str) -> Option<Noted> {
    try_noted(storage, base).ok().flatten()
}

/// A line of the list, if it notes picture `name` at a size it can have.
fn parse(line: &str, name: &str) -> Result<Option<Noted>, DocumentError> {
    let mut fields = line.split('\t');
    if fields.next() != Some(name) {
        return Ok(None);
    }
    let size = fields
        .next()
        .and_then(|w| w.parse().ok())
        .filter(|&w| w > 0)
        .zip(
            fields
                .next()
                .and_then(|h| h.parse().ok())
                .filter(|&h| h > 0),
        );
    let (Some(size), Some(source)) = (size, fields.next()) else {
        return Ok(None);
    };
    Ok(Some((size, crate::fallible::string(source)?)))
}

/// Picture `base`: decoded, else as noted, to be decoded in the kind
/// wanted, unless it was found not to be decodable. No room is
/// `OutOfMemory`.
pub(crate) fn open_or_noted(
    storage: &dyn Storage,
    base: &str,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    if let Some(image) = try_open(storage, base, color)? {
        return Ok(Some(image));
    }
    if failed(storage, base) {
        return Ok(None);
    }
    let Some(((width, height), _)) = try_noted(storage, base)? else {
        return Ok(None);
    };
    Ok(Some(ImageRef {
        path: try_file(base, color)?,
        width,
        height,
    }))
}
/// Whether picture `base` is decoded in the kind wanted.
pub(crate) fn has(storage: &dyn Storage, base: &str, color: bool) -> Result<bool, DocumentError> {
    Ok(ImageFile::open(storage, &try_file(base, color)?).is_some())
}
