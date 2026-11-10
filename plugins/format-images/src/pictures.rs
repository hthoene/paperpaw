//! A picture of a page decoded, on its page, into the cache: a few rows
//! at a time, no larger than the screen shows, so one page's picture is in
//! memory at most, and a hostile file is a page that is named, not shown.

use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{DocumentError, ImageRef};
use paperpaw_sdk::image::{ColorWriter, Fit, Format, GrayWriter, ImageError, Source, decode};

use crate::MAX_FILE;

/// The parts joined; an error instead of an abort when memory is short.
pub(crate) fn text(parts: &[&str]) -> Result<String, DocumentError> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
        .map_err(|_| DocumentError::OutOfMemory)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

/// `value` in decimal (or `digits` hexadecimal ones of it), on the stack.
pub(crate) fn decimal(value: u64, out: &mut [u8; 20]) -> &str {
    number(value, 10, out)
}

fn number(mut value: u64, radix: u64, out: &mut [u8; 20]) -> &str {
    let mut at = out.len();
    loop {
        at -= 1;
        let digit = (value % radix) as u8;
        out[at] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        };
        value /= radix;
        if value == 0 {
            break;
        }
    }
    core::str::from_utf8(&out[at..]).unwrap_or_default()
}

/// The cache file of the picture of page `index`, `name` of `size` bytes:
/// another file under the same name is another picture.
pub(crate) fn base(
    cache: &str,
    index: usize,
    name: &str,
    size: u64,
) -> Result<String, DocumentError> {
    let mut bytes = alloc::vec::Vec::new();
    bytes
        .try_reserve_exact(name.len() + 8)
        .map_err(|_| DocumentError::OutOfMemory)?;
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    let tag = u64::from(paperpaw_sdk::digest::fnv1a64(bytes) as u32);
    let (mut a, mut b) = ([0u8; 20], [0u8; 20]);
    let index = number(index as u64, 10, &mut a);
    let tag = number(tag, 16, &mut b);
    // Eight hex digits, as before.
    let pad = &"00000000"[..8usize.saturating_sub(tag.len())];
    join(cache, &["p", index, "-", pad, tag])
}

/// The prefix of the paths of the document cache (root 6) among the paths of
/// the book's folder (root 3) in the native provider's one storage: a colon is
/// no part of a name on the card.
pub const CACHE: &str = "cache:";

/// `leaf` (in parts) in the folder `cache` (a path, or [`CACHE`] for the host's).
pub(crate) fn join(cache: &str, leaf: &[&str]) -> Result<String, DocumentError> {
    let mut out = String::new();
    let room = cache.len() + 1 + leaf.iter().map(|p| p.len()).sum::<usize>();
    out.try_reserve_exact(room)
        .map_err(|_| DocumentError::OutOfMemory)?;
    out.push_str(cache);
    if !cache.ends_with(':') {
        out.push('/');
    }
    for part in leaf {
        out.push_str(part);
    }
    Ok(out)
}

fn file(base: &str, color: bool) -> Result<String, DocumentError> {
    text(&[base, ".", if color { "rgb" } else { "gray" }])
}

/// The mark of a picture that cannot be decoded.
fn mark(base: &str) -> Result<String, DocumentError> {
    text(&[base, ".bad"])
}

/// A file as a source of image bytes.
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

/// The picture `base` as decoded, if it is: the header the writers made
/// (`PPGI` gray or `PPCI` colour, width and height), and the file of exactly
/// the size it names. Read here rather than by `ImageFile::open`, which makes
/// its path strings without a way to refuse (a failure of memory is the
/// caller's, and is tried again).
pub(crate) fn open(
    storage: &dyn Storage,
    base: &str,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    let path = file(base, color)?;
    let mut header = [0u8; 12];
    // A read may return less than asked: fill the header, as `read_all` does.
    let mut filled = 0;
    while filled < header.len() {
        match storage.read_at(&path, filled as u64, &mut header[filled..]) {
            Ok(0) | Err(paperpaw_sdk::StorageError::NotFound) => return Ok(None),
            Ok(n) => filled += n,
            Err(error) => return Err(DocumentError::Storage(error)),
        }
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
    match (storage.size(&path), expected) {
        (Ok(size), Some(expected)) if size == expected => Ok(Some(ImageRef {
            path,
            width,
            height,
        })),
        (Err(error), _) if error != paperpaw_sdk::StorageError::NotFound => {
            Err(DocumentError::Storage(error))
        }
        _ => Ok(None),
    }
}

/// The picture in the file `source` (of `size` bytes) as shown on its page:
/// decoded into `base` when it is not yet, no larger than `fit`. A file that
/// is no picture, or too large, is marked, and not tried again (`None`); a
/// failure of the card or of memory is the caller's, and is tried again.
pub(crate) fn shown(
    storage: &dyn Storage,
    source: &str,
    size: u64,
    base: &str,
    fit: Fit,
    color: bool,
) -> Result<Option<ImageRef>, DocumentError> {
    if let Some(image) = open(storage, base, color)? {
        return Ok(Some(image));
    }
    let bad = mark(base)?;
    if storage.exists(&bad) {
        return Ok(None);
    }
    if size == 0 || size > MAX_FILE {
        storage.write(&bad, &[]).ok();
        return Ok(None);
    }
    let mut head = [0u8; 8];
    let n = storage.read_at(source, 0, &mut head)?;
    let Some(format) = Format::detect(&head[..n]) else {
        storage.write(&bad, &[]).ok();
        return Ok(None);
    };
    let to = file(base, color)?;
    let mut input = Whole {
        storage,
        path: source,
        offset: 0,
    };
    let result = if color {
        let mut writer = ColorWriter::new(storage, &to);
        decode(format, &mut input, fit, &mut writer).and_then(|()| writer.finish())
    } else {
        let mut writer = GrayWriter::new(storage, &to);
        decode(format, &mut input, fit, &mut writer).and_then(|()| writer.finish())
    };
    match result {
        Ok(()) => open(storage, base, color),
        Err(error) => {
            storage.remove(&to).ok();
            match error {
                ImageError::OutOfMemory => Err(DocumentError::OutOfMemory),
                ImageError::Io => Err(DocumentError::Storage(paperpaw_sdk::StorageError::Io)),
                ImageError::Storage(e) => Err(DocumentError::Storage(e)),
                _ => {
                    storage.write(&bad, &[]).ok();
                    Ok(None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::open;
    use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

    /// A storage that gives at most `step` bytes for a read, as a card may.
    struct Short(MemoryStorage, usize);

    impl Storage for Short {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            let n = buf.len().min(self.1);
            self.0.read_at(path, at, &mut buf[..n])
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.0.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.append(path, data)
        }
        fn list(&self, dir: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(dir, v)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.0.remove(path)
        }
    }

    fn file(magic: &[u8], width: u32, height: u32, pixels: usize) -> alloc::vec::Vec<u8> {
        let mut out = magic.to_vec();
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.resize(12 + pixels, 0);
        out
    }

    fn opened(data: alloc::vec::Vec<u8>, color: bool) -> Option<(u32, u32)> {
        let name = if color { "b.rgb" } else { "b.gray" };
        let io = MemoryStorage::new().with(name, data);
        open(&io, "b", color)
            .unwrap()
            .map(|image| (image.width, image.height))
    }

    #[test]
    fn a_decoded_picture_opens_with_its_size() {
        assert_eq!(opened(file(b"PPGI", 3, 2, 6), false), Some((3, 2)));
        assert_eq!(opened(file(b"PPCI", 3, 2, 18), true), Some((3, 2)));
    }

    #[test]
    fn a_read_that_gives_less_is_read_again() {
        let io = Short(
            MemoryStorage::new().with("b.gray", file(b"PPGI", 3, 2, 6)),
            5,
        );
        assert_eq!(open(&io, "b", false).unwrap().map(|i| i.width), Some(3));
    }

    #[test]
    fn a_hostile_header_is_no_picture() {
        // Wrong magic, and the other kind's magic.
        assert_eq!(opened(file(b"XXXX", 3, 2, 6), false), None);
        assert_eq!(opened(file(b"PPCI", 3, 2, 6), false), None);
        // No pixels at all, or no width or height.
        assert_eq!(opened(file(b"PPGI", 0, 2, 0), false), None);
        assert_eq!(opened(file(b"PPGI", 3, 0, 0), false), None);
        // Sizes that no file has (and whose product overflows).
        assert_eq!(opened(file(b"PPGI", u32::MAX, u32::MAX, 0), false), None);
        assert_eq!(opened(file(b"PPCI", u32::MAX, u32::MAX, 0), true), None);
        assert_eq!(opened(file(b"PPGI", u32::MAX, 1, 6), false), None);
        // One byte too many, or too few.
        assert_eq!(opened(file(b"PPGI", 3, 2, 7), false), None);
        assert_eq!(opened(file(b"PPGI", 3, 2, 5), false), None);
        assert_eq!(opened(file(b"PPCI", 3, 2, 17), true), None);
    }

    #[test]
    fn a_short_or_missing_file_is_no_picture() {
        assert_eq!(opened(alloc::vec::Vec::new(), false), None);
        assert_eq!(opened(b"PPGI".to_vec(), false), None);
        assert_eq!(opened(b"PPGI\x03\0\0\0\x02\0\0".to_vec(), false), None);
        let io = MemoryStorage::new();
        assert!(open(&io, "b", false).unwrap().is_none());
    }
}
