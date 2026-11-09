//! Comic book archives (`.cbz`): a ZIP of pictures, one page each, in the
//! order of their names (numbers counted as numbers, so `2.jpg` comes
//! before `10.jpg`). A `ComicInfo.xml` gives title, series, number,
//! writers and summary.
//!
//! Each picture is a section of one image block; it is decoded when its
//! page is first shown and kept in the cache, so only one page is ever
//! decoded at a time, also on small devices.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{
    Align, Block, BlockKind, Document, DocumentError, ImageRef, Locator, Metadata, OffsetMap,
};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::xml::{Token, Tokenizer, local_name};
use paperpaw_sdk::zip::ZipArchive;

use crate::PLUGIN;
use crate::cached;
use crate::convert;
use crate::fallible;
use crate::package::error;

/// Pages kept at most: the first in reading order.
const MAX_PAGES: usize = 5000;
/// Memory for picture names while the pages are put in order (a name and
/// what holds it, about 16 bytes on the X4), and the longest name taken
/// (no page is named longer).
const MAX_NAMES: usize = 128 * 1024;
const ENTRY: usize = 16;
const MAX_NAME: usize = 1024;
/// Marks a book without `ComicInfo.xml` in `pages.bin`.
const NO_INFO: u32 = u32::MAX;
/// Largest `ComicInfo.xml` read.
const MAX_INFO: usize = 64 * 1024;

/// An open comic book.
#[derive(Debug)]
pub struct CbzDocument {
    archive: ZipArchive,
    /// The number of pages; where each is in the archive is in the cache
    /// (`pages.bin`: the record of `ComicInfo.xml`, then the pages' in
    /// reading order, a u32 each), so the names are put in order once.
    pages: u32,
    cache: String,
    metadata: Metadata,
    /// Pages in colour (ADR 0015).
    color: bool,
    /// Pages decoded at most this large.
    image_fit: Fit,
    cover_fit: Fit,
}

/// Whether `name` is a picture PaperPaw shows.
fn is_picture(name: &str) -> bool {
    let bytes = name.as_bytes();
    let starts = |prefix: &str| {
        bytes
            .get(..prefix.len())
            .is_some_and(|b| b.eq_ignore_ascii_case(prefix.as_bytes()))
    };
    let ends = |suffix: &str| {
        bytes
            .len()
            .checked_sub(suffix.len())
            .is_some_and(|at| bytes[at..].eq_ignore_ascii_case(suffix.as_bytes()))
    };
    !starts("__macosx/")
        && !name.rsplit('/').next().unwrap_or("").starts_with('.')
        && [".jpg", ".jpeg", ".png", ".gif"].iter().any(|e| ends(e))
}

/// Names in reading order: numbers within them by value.
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let digits = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
                let (n, m) = (digits(a), digits(b));
                let trim = |s: &[u8]| -> usize { s.iter().take_while(|&&c| c == b'0').count() };
                let (x0, y0) = (trim(&a[..n]).min(n - 1), trim(&b[..m]).min(m - 1));
                let (x, y) = (&a[x0..n], &b[y0..m]);
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[n..];
                b = &b[m..];
            }
            (Some(x), Some(y)) => {
                let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

impl CbzDocument {
    pub fn open(storage: &dyn Storage, path: &str) -> Result<Self, DocumentError> {
        Self::open_options(storage, path, &paperpaw_sdk::OpenOptions::default())
    }

    /// Opens a comic as `options` say (pages in colour or gray).
    pub fn open_options(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
    ) -> Result<Self, DocumentError> {
        let archive = ZipArchive::open(storage, path).map_err(error)?;
        let cache = fallible::kept_path(&fallible::plugin_dir(PLUGIN.as_str())?, "cbz-", path)?;
        // Pictures decoded from another file of the same name go.
        let stamp = fallible::try_format!("{}\n", fallible::fingerprint(storage, path)?)?;
        let stamp_path = fallible::try_format!("{cache}/source")?;
        let list = fallible::try_format!("{cache}/pages.bin")?;
        let fresh = paperpaw_sdk::read_file(storage, &stamp_path, 64)
            .ok()
            .as_deref()
            == Some(stamp.as_bytes());
        let pages = match storage.size(&list) {
            Ok(size) if fresh && size >= 8 => (size / 4 - 1) as u32,
            _ => {
                let _ = storage.remove_dir_all(&cache);
                let _ = storage.create_dir(&cache);
                let pages = list_pages(storage, &archive, &list)?;
                storage.write(&stamp_path, stamp.as_bytes())?;
                pages
            }
        };
        let info = cached::u32_at(storage, &list, 0)
            .ok()
            .filter(|&record| record != NO_INFO)
            .and_then(|record| archive.entry_by_record(storage, record).ok());
        let mut metadata = match info {
            Some(entry) => comic_info(storage, &archive, &entry)?.unwrap_or_default(),
            None => Metadata::default(),
        };
        if metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(fallible::string(
                name.rsplit_once('.').map_or(name, |(s, _)| s),
            )?);
        }
        Ok(Self {
            archive,
            pages,
            cache,
            metadata,
            color: options.color,
            image_fit: options.image_fit,
            cover_fit: options.cover_fit,
        })
    }

    /// The archive entry of page `index`.
    fn page(
        &self,
        storage: &dyn Storage,
        index: usize,
    ) -> Result<Option<paperpaw_sdk::zip::Entry>, DocumentError> {
        let at = (index as u64 + 1) * 4;
        let list = fallible::try_format!("{}/pages.bin", self.cache)?;
        let Ok(record) = cached::u32_at(storage, &list, at) else {
            return Ok(None);
        };
        match self.archive.entry_by_record(storage, record) {
            Ok(entry) => Ok(Some(entry)),
            Err(e) => match error(e) {
                DocumentError::OutOfMemory => Err(DocumentError::OutOfMemory),
                _ => Ok(None),
            },
        }
    }

    /// Page `index` decoded no larger than `fit` allows into
    /// the picture `base` (see `pictures`), if it can be.
    fn picture(
        &self,
        storage: &dyn Storage,
        index: usize,
        base: &str,
        fit: Fit,
    ) -> Result<Option<ImageRef>, DocumentError> {
        if !crate::pictures::has(storage, base, self.color)? {
            if crate::pictures::failed(storage, base) {
                return Ok(None);
            }
            let Some(entry) = self.page(storage, index)? else {
                return Ok(None);
            };
            let file = crate::pictures::try_file(base, self.color)?;
            // A picture that cannot be decoded is its page's name; one that
            // found no room is that too, and is tried again.
            if convert::decode_entry(
                storage,
                &self.archive,
                &entry,
                &file,
                &self.cache,
                fit,
                self.color,
            )
            .inspect_err(|e| crate::pictures::note_failure(storage, base, e))
            .is_err()
            {
                return Ok(None);
            }
        }
        crate::pictures::try_open(storage, base, self.color)
    }
}

/// Writes `pages.bin` to `list`: the record of `ComicInfo.xml`, then those
/// of the pictures in reading order, the first [`MAX_PAGES`]. Their names
/// are held in [`MAX_NAMES`] bytes: past that, the last in reading order
/// go, so those kept are always the first. How many pages.
fn list_pages(
    storage: &dyn Storage,
    archive: &ZipArchive,
    list: &str,
) -> Result<u32, DocumentError> {
    let mut pages: Vec<(String, u32)> = Vec::new();
    let mut bytes = 0;
    let mut info = NO_INFO;
    let keep = |pages: &mut Vec<(String, u32)>, count: usize| {
        paperpaw_sdk::sort::sort_by(pages, |a, b| natural(&a.0, &b.0));
        pages.truncate(count);
        pages
            .iter()
            .map(|(name, _)| name.len() + ENTRY)
            .sum::<usize>()
    };
    for entry in archive.entries(storage) {
        let entry = entry.map_err(error)?;
        if entry.name.eq_ignore_ascii_case("ComicInfo.xml") {
            info = entry.record();
        } else if is_picture(&entry.name) && entry.name.len() <= MAX_NAME {
            bytes += entry.name.len() + ENTRY;
            let record = entry.record();
            fallible::push(&mut pages, (entry.name, record))?;
            if pages.len() > 2 * MAX_PAGES {
                bytes = keep(&mut pages, MAX_PAGES);
            } else if bytes > MAX_NAMES {
                let count = if pages.len() > MAX_PAGES {
                    MAX_PAGES
                } else {
                    pages.len() * 3 / 4
                };
                bytes = keep(&mut pages, count.max(1));
            }
        }
    }
    keep(&mut pages, MAX_PAGES);
    if pages.is_empty() {
        return Err(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "no pictures",
        )));
    }
    let mut data = Vec::new();
    fallible::reserve_exact(&mut data, 4 * (1 + pages.len()))?;
    data.extend_from_slice(&info.to_le_bytes());
    for (_, record) in &pages {
        data.extend_from_slice(&record.to_le_bytes());
    }
    storage.write(list, &data)?;
    Ok(pages.len() as u32)
}

/// Title, series and number, writers, summary, language and genres from
/// `ComicInfo.xml`; none where it cannot be read, `OutOfMemory` where it
/// finds no room.
fn comic_info(
    storage: &dyn Storage,
    archive: &ZipArchive,
    entry: &paperpaw_sdk::zip::Entry,
) -> Result<Option<Metadata>, DocumentError> {
    let mut data: Vec<u8> = Vec::new();
    let mut refused = None;
    let read = archive.stream(storage, entry, &mut |chunk| {
        if let Err(e) = fallible::reserve(&mut data, chunk.len()) {
            refused = Some(e);
            return false;
        }
        data.extend_from_slice(chunk);
        data.len() < MAX_INFO
    });
    if let Some(e) = refused {
        return Err(e);
    }
    if read.is_err() {
        return Ok(None);
    }
    let mut metadata = Metadata::default();
    let mut field = String::new();
    let mut text = String::new();
    let list = |text: &str| -> Result<Vec<String>, DocumentError> {
        let mut out = Vec::new();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            fallible::push(&mut out, fallible::string(item)?)?;
        }
        Ok(out)
    };
    let mut fill = |token: Token<'_>| -> Result<(), DocumentError> {
        match token {
            Token::Start { name, .. } => {
                field = fallible::string(local_name(name))?;
                text.clear();
            }
            Token::Text(t) => {
                let decoded = paperpaw_sdk::xml::try_decode(&t).map_err(|_| fallible::OOM)?;
                fallible::push_str(&mut text, &decoded)?;
            }
            Token::End { name } => {
                let value = text.trim();
                if local_name(name) == field && !value.is_empty() {
                    match field.as_str() {
                        "Title" => metadata.title = Some(fallible::string(value)?),
                        "Series" => metadata.series = Some(fallible::string(value)?),
                        "Number" => metadata.series_index = Some(fallible::string(value)?),
                        "Writer" => metadata.authors = list(value)?,
                        "Summary" => metadata.description = Some(fallible::string(value)?),
                        "LanguageISO" => metadata.language = Some(fallible::string(value)?),
                        "Genre" | "Tags" => {
                            for subject in list(value)? {
                                fallible::push(&mut metadata.subjects, subject)?;
                            }
                        }
                        _ => {}
                    }
                }
                text.clear();
            }
            _ => {}
        }
        Ok(())
    };
    let mut failed = None;
    Tokenizer::parse(&data, &mut |token| {
        if failed.is_none()
            && let Err(e) = fill(token)
        {
            failed = Some(e);
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    // An issue's title alone says little: with its series.
    if let (Some(series), Some(title)) = (&metadata.series, &metadata.title)
        && !title.contains(series.as_str())
    {
        metadata.title = Some(fallible::try_format!("{series}: {title}")?);
    } else if metadata.title.is_none()
        && let Some(series) = &metadata.series
    {
        metadata.title = Some(match &metadata.series_index {
            Some(number) => fallible::try_format!("{series} {number}")?,
            None => fallible::string(series)?,
        });
    }
    Ok(Some(metadata))
}

impl Document for CbzDocument {
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
        let index = at.section as usize;
        if index >= self.pages as usize || at.offset >= 1 {
            return Ok(None);
        }
        let cache = fallible::try_format!("{}/p{index}", self.cache)?;
        let image = self.picture(storage, index, &cache, self.image_fit)?;
        // A picture that cannot be shown is named instead.
        let (kind, text) = match &image {
            Some(_) => (BlockKind::Image, String::new()),
            None => (
                BlockKind::Paragraph,
                self.page(storage, index)?
                    .map(|e| e.name)
                    .unwrap_or_default(),
            ),
        };
        Ok(Some(
            Block::new(kind, 0, 1, text)
                .map(OffsetMap::IDENTITY)
                .align(Align::Center)
                .image(image),
        ))
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        let cache = fallible::try_format!("{}/cover-{}", self.cache, self.cover_fit.long)?;
        self.picture(storage, 0, &cache, self.cover_fit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_zip::testing::ZipBuilder;

    #[test]
    fn pages_in_natural_order() {
        let mut names = ["p10.jpg", "p2.jpg", "P1.jpg", "p02b.jpg"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["P1.jpg", "p2.jpg", "p02b.jpg", "p10.jpg"]);
        assert!(is_picture("a/1.PNG"));
        assert!(!is_picture("__MACOSX/1.png"));
        assert!(!is_picture("ComicInfo.xml"));
    }

    #[test]
    fn opens_pages_and_comic_info() {
        let info = "<?xml version=\"1.0\"?><ComicInfo><Title>The Start</Title>\
            <Series>Tales</Series><Number>1</Number><Writer>Ann, Bob</Writer>\
            <Summary>It begins.</Summary></ComicInfo>";
        let zip = ZipBuilder::new()
            .stored("10.jpg", b"not a picture".to_vec())
            .stored("2.jpg", b"not one either".to_vec())
            .deflated("ComicInfo.xml", info.as_bytes())
            .build();
        let storage = MemoryStorage::new().with("c/t.cbz", zip);
        let doc = CbzDocument::open(&storage, "c/t.cbz").unwrap();
        let m = doc.metadata();
        assert_eq!(m.title.as_deref(), Some("Tales: The Start"));
        assert_eq!(m.authors, ["Ann", "Bob"]);
        assert_eq!(m.series_index.as_deref(), Some("1"));
        assert_eq!(m.description.as_deref(), Some("It begins."));
        assert_eq!(doc.sections(), 2);
        // Pictures that cannot be decoded are named.
        let block = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
        assert_eq!(block.text, "2.jpg");
        assert!(
            doc.block_at(&storage, Locator::new(0, 1))
                .unwrap()
                .is_none()
        );
        assert!(
            doc.block_at(&storage, Locator::new(2, 0))
                .unwrap()
                .is_none()
        );
    }
}
