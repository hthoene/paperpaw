//! Comic book archives (`.cbz`): a ZIP of pictures, one page each, in the
//! order of their names (numbers counted as numbers, so `2.jpg` comes
//! before `10.jpg`). A `ComicInfo.xml` gives title, series, number,
//! writers, summary and whether the book is manga (read from right to left).
//!
//! In panel mode (the setting [`PANELS`]) a page is as many positions as it
//! has panels (found by `cbz_panels`), each a picture of its own, the panel
//! as large as the page's size allows; a page without panels is one position.
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
    PageProgression,
};
use paperpaw_sdk::image::{Fit, GrayFile, ImageFile};
use paperpaw_sdk::xml::{Token, Tokenizer, local_name};
use paperpaw_sdk::zip::ZipArchive;

use crate::PLUGIN;
use crate::cached;
use crate::cbz_panels::{self, MAX_PANELS, Panel};
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
/// Most of `ComicInfo.xml` read (more is left unread).
const MAX_INFO: usize = 256 * 1024;
/// Most of a field's text kept (a longer summary is cut).
const MAX_FIELD: usize = 4096;
/// The longest element name looked at.
const MAX_FIELD_NAME: usize = 32;

/// The setting that makes pages panels (a switch, off to begin with).
pub const PANELS: &str = "format-cbz.panels";
/// The setting of the way pages run: from the book (`ComicInfo.xml`'s
/// `Manga`), left to right or right to left.
pub const DIRECTION: &str = "format-cbz.direction";
/// The choice of [`DIRECTION`] that reads right to left.
pub const RIGHT_TO_LEFT: &str = "format-cbz.direction.rtl";
/// The choice of [`DIRECTION`] that reads left to right.
pub const LEFT_TO_RIGHT: &str = "format-cbz.direction.ltr";

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
    /// Pages are shown panel by panel.
    panels: bool,
    /// Pages and panels run from right to left.
    rtl: bool,
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
        // The setting wins over the book's `Manga`.
        let rtl = match options.setting(DIRECTION) {
            Some(RIGHT_TO_LEFT) => true,
            Some(LEFT_TO_RIGHT) => false,
            _ => metadata.direction == PageProgression::RightToLeft,
        };
        metadata.direction = if rtl {
            PageProgression::RightToLeft
        } else {
            PageProgression::LeftToRight
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
            panels: matches!(options.setting(PANELS), Some("true" | "on" | "1")),
            rtl,
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

impl CbzDocument {
    /// The letter that tells the lists and pictures of a direction apart.
    fn way(&self) -> char {
        if self.rtl { 'r' } else { 'l' }
    }

    /// The panels of page `index`, in reading order, from its list or found
    /// now in `page` (its decoded gray picture) and kept; none where the
    /// page has none (or is in colour).
    fn panels_of(
        &self,
        storage: &dyn Storage,
        index: usize,
        page: &ImageRef,
    ) -> Result<Vec<Panel>, DocumentError> {
        if !page.path.ends_with(".gray") {
            return Ok(Vec::new());
        }
        let list = fallible::try_format!("{}/p{index}.{}.panels", self.cache, self.way())?;
        let size = storage.size(&list).unwrap_or(0);
        if size > 0
            && size <= 1 + (MAX_PANELS * 8) as u64
            && let Ok(bytes) = paperpaw_sdk::read_file(storage, &list, 1 + MAX_PANELS * 8)
        {
            return cbz_panels::decode(&bytes);
        }
        let Some(file) = GrayFile::open(storage, &page.path) else {
            return Ok(Vec::new());
        };
        let found = match cbz_panels::find(storage, &file, self.rtl) {
            Ok(found) => found,
            Err(DocumentError::OutOfMemory) => return Err(DocumentError::OutOfMemory),
            // A page that cannot be read has no panels now; tried again later.
            Err(_) => return Ok(Vec::new()),
        };
        // Not kept where there is no room: found again next time.
        let _ = storage.write(&list, &cbz_panels::encode(&found)?);
        Ok(found)
    }

    /// The picture file of panel `k` of page `index`.
    fn crop_path(&self, index: usize, k: usize) -> Result<String, DocumentError> {
        fallible::try_format!("{}/q{index}.{k}{}.gray", self.cache, self.way())
    }

    /// Draws the panel `image` names (a file made by [`Self::crop_path`]): the
    /// pictures of the page shown before go, so a book keeps the panels of
    /// one page.
    fn make_crop(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        let name = image
            .path
            .strip_prefix(self.cache.as_str())?
            .strip_prefix("/q")?;
        let name = name.strip_suffix(".gray")?;
        let (index, rest) = name.split_once('.')?;
        let (k, way) = rest.split_at(rest.len().checked_sub(1)?);
        let (index, k): (usize, usize) = (index.parse().ok()?, k.parse().ok()?);
        if way != self.way().encode_utf8(&mut [0; 4]) || index >= self.pages as usize {
            return None;
        }
        let base = fallible::try_format!("{}/p{index}", self.cache).ok()?;
        let page = crate::pictures::open(storage, &base, false)?;
        let panel = *self.panels_of(storage, index, &page).ok()?.get(k)?;
        let file = GrayFile::open(storage, &page.path)?;
        let marker = fallible::try_format!("{}/q.page", self.cache).ok()?;
        let shown = paperpaw_sdk::read_file(storage, &marker, 16).ok();
        let number = fallible::try_format!("{index}").ok()?;
        if shown.as_deref() != Some(number.as_bytes()) {
            if let Some(old) = shown
                .as_deref()
                .and_then(|b| core::str::from_utf8(b).ok())
                .and_then(|t| t.parse::<usize>().ok())
            {
                for k in 0..MAX_PANELS {
                    if let Ok(path) = self.crop_path(old, k) {
                        let _ = storage.remove(&path);
                    }
                }
            }
            storage.write(&marker, number.as_bytes()).ok()?;
        }
        cbz_panels::crop(storage, &file, panel, &image.path).ok()?;
        crate::pictures::open_path(storage, &image.path)
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

/// What is read of `ComicInfo.xml` while it is streamed: the text of the
/// element being read among the root's children, and the metadata so far.
struct Info {
    metadata: Metadata,
    depth: usize,
    field: String,
    text: String,
}

impl Info {
    fn list(text: &str) -> Result<Vec<String>, DocumentError> {
        let mut out = Vec::new();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            fallible::push(&mut out, fallible::string(item)?)?;
        }
        Ok(out)
    }

    /// Adds text of the field being read, cut at [`MAX_FIELD`] bytes.
    fn add(&mut self, text: &str) -> Result<(), DocumentError> {
        if self.depth != 2 || self.field.is_empty() {
            return Ok(());
        }
        let room = MAX_FIELD.saturating_sub(self.text.len());
        let mut end = text.len().min(room);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        fallible::push_str(&mut self.text, &text[..end])
    }

    fn token(&mut self, token: Token<'_>) -> Result<(), DocumentError> {
        match token {
            Token::Start {
                name, self_closing, ..
            } => {
                if !self_closing {
                    self.depth += 1;
                }
                self.field.clear();
                self.text.clear();
                let name = local_name(name);
                if self.depth == 2 && !self_closing && name.len() <= MAX_FIELD_NAME {
                    fallible::push_str(&mut self.field, name)?;
                }
            }
            Token::Text(t) => {
                let decoded = paperpaw_sdk::xml::try_decode(&t).map_err(|_| fallible::OOM)?;
                self.add(&decoded)?;
            }
            Token::CData(t) => self.add(t)?,
            Token::End { name } => {
                if self.depth == 2 && local_name(name) == self.field {
                    self.commit()?;
                }
                self.depth = self.depth.saturating_sub(1);
                self.field.clear();
                self.text.clear();
            }
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DocumentError> {
        let value = self.text.trim();
        if value.is_empty() {
            return Ok(());
        }
        let metadata = &mut self.metadata;
        match self.field.as_str() {
            "Title" => metadata.title = Some(fallible::string(value)?),
            "Series" => metadata.series = Some(fallible::string(value)?),
            "Number" => metadata.series_index = Some(fallible::string(value)?),
            "Writer" => metadata.authors = Self::list(value)?,
            "Summary" => metadata.description = Some(fallible::string(value)?),
            "LanguageISO" => metadata.language = Some(fallible::string(value)?),
            "Manga" if value.eq_ignore_ascii_case("YesAndRightToLeft") => {
                metadata.direction = PageProgression::RightToLeft;
            }
            "Genre" | "Tags" => {
                for subject in Self::list(value)? {
                    fallible::push(&mut metadata.subjects, subject)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Title, series and number, writers, summary, language, genres and
/// whether it is manga from `ComicInfo.xml`; none where it cannot be read,
/// `OutOfMemory` where it finds no room. The file is streamed through the
/// XML tokenizer: no more than a few KiB are held, whatever its size, and
/// no more than [`MAX_INFO`] bytes are read.
fn comic_info(
    storage: &dyn Storage,
    archive: &ZipArchive,
    entry: &paperpaw_sdk::zip::Entry,
) -> Result<Option<Metadata>, DocumentError> {
    let mut info = Info {
        metadata: Metadata::default(),
        depth: 0,
        field: String::new(),
        text: String::new(),
    };
    let mut tokenizer = Tokenizer::new();
    let mut failed = None;
    let mut read = 0;
    let streamed = archive.stream(storage, entry, &mut |chunk| {
        read += chunk.len();
        let fed = tokenizer.try_feed(chunk, &mut |token| {
            if failed.is_none()
                && let Err(e) = info.token(token)
            {
                failed = Some(e);
            }
        });
        if fed.is_err() {
            failed = Some(fallible::OOM);
        }
        failed.is_none() && read < MAX_INFO
    });
    if let Some(e) = failed {
        return Err(e);
    }
    if streamed.is_err() {
        return Ok(None);
    }
    tokenizer.finish(&mut |token| {
        if failed.is_none()
            && let Err(e) = info.token(token)
        {
            failed = Some(e);
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    let mut metadata = info.metadata;
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
        if self.panels { MAX_PANELS as u32 } else { 1 }
    }

    fn pictures_deferred(&self) -> bool {
        self.panels
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let index = at.section as usize;
        if index >= self.pages as usize || (!self.panels && at.offset >= 1) {
            return Ok(None);
        }
        let cache = fallible::try_format!("{}/p{index}", self.cache)?;
        let page = self.picture(storage, index, &cache, self.image_fit)?;
        let panels = match (&page, self.panels) {
            (Some(page), true) => self.panels_of(storage, index, page)?,
            _ => Vec::new(),
        };
        let k = at.offset as usize;
        if k >= panels.len().max(1) {
            return Ok(None);
        }
        // A panel is a picture of its own, of the page's size, made when a
        // page shows it (`prepare_image`).
        let image = match (panels.get(k), &page) {
            (Some(_), Some(page)) => Some(ImageRef {
                path: self.crop_path(index, k)?,
                width: page.width,
                height: page.height,
            }),
            _ => page,
        };
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
            Block::new(kind, k as u32, k as u32 + 1, text)
                .map(OffsetMap::IDENTITY)
                .align(Align::Center)
                .image(image),
        ))
    }

    fn prepare_image(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        if !self.panels || ImageFile::open(storage, &image.path).is_some() {
            return None;
        }
        self.make_crop(storage, image)
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
