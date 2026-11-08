//! A picture of a page decoded, on its page, into the cache: a few rows
//! at a time, no larger than the screen shows, so one page's picture is in
//! memory at most, and a hostile file is a page that is named, not shown.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::image::{
    ColorWriter, Fit, Format, GrayWriter, ImageError, ImageFile, Source, decode,
};

use crate::MAX_FILE;

/// The cache file of the picture of page `index`, `name` of `size` bytes:
/// another file under the same name is another picture.
pub(crate) fn base(cache: &str, index: usize, name: &str, size: u64) -> String {
    let mut bytes = alloc::vec::Vec::from(name.as_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    let tag = paperpaw_sdk::digest::fnv1a64(bytes) as u32;
    join(cache, &format!("p{index}-{tag:08x}"))
}

/// The prefix of the paths of the document cache (root 6) among the paths of
/// the book's folder (root 3) in the native provider's one storage: a colon is
/// no part of a name on the card.
pub const CACHE: &str = "cache:";

/// `leaf` in the folder `cache` (a path, or [`CACHE`] for the host's).
pub(crate) fn join(cache: &str, leaf: &str) -> String {
    if cache.ends_with(':') {
        format!("{cache}{leaf}")
    } else {
        format!("{cache}/{leaf}")
    }
}

fn file(base: &str, color: bool) -> String {
    format!("{base}.{}", if color { "rgb" } else { "gray" })
}

/// The mark of a picture that cannot be decoded.
fn mark(base: &str) -> String {
    format!("{base}.bad")
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

/// The picture `base` as decoded, if it is.
pub(crate) fn open(storage: &dyn Storage, base: &str, color: bool) -> Option<ImageRef> {
    ImageFile::open(storage, &file(base, color))
        .filter(|image| image.width > 0 && image.height > 0)
        .map(|image| ImageRef {
            path: image.path,
            width: image.width,
            height: image.height,
        })
}

/// The picture in the file `source` (of `size` bytes) as shown on its page:
/// decoded into `base` when it is not yet, no larger than `fit`. A file that
/// is no picture, or too large, is marked, and not tried again; a failure of
/// the card or of memory may pass, and is tried again.
pub(crate) fn shown(
    storage: &dyn Storage,
    source: &str,
    size: u64,
    base: &str,
    fit: Fit,
    color: bool,
) -> Option<ImageRef> {
    if let Some(image) = open(storage, base, color) {
        return Some(image);
    }
    if storage.exists(&mark(base)) {
        return None;
    }
    if size == 0 || size > MAX_FILE {
        storage.write(&mark(base), &[]).ok();
        return None;
    }
    let mut head = [0u8; 8];
    let n = storage.read_at(source, 0, &mut head).ok()?;
    let Some(format) = Format::detect(&head[..n]) else {
        storage.write(&mark(base), &[]).ok();
        return None;
    };
    let to = file(base, color);
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
            if !matches!(
                error,
                ImageError::Io | ImageError::Storage(_) | ImageError::OutOfMemory
            ) {
                storage.write(&mark(base), &[]).ok();
            }
            None
        }
    }
}
