//! A Word document as a [`Document`]: converted once into the cache (see
//! [`crate::cache`]), read from there.
//!
//! The native provider's document ([`DocxDocument::native`]): the archive is
//! the book (root 3), the cache is root 6; both paths are "", the cache's
//! files are named by themselves. The conversion ([`DocxDocument::convert`])
//! and the reading are one code; every allocation in them can be refused
//! (`OutOfMemory`).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{
    Block, BlockKind, Document, DocumentError, ImageRef, Link, Locator, Metadata, TocEntry,
};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::zip::ZipArchive;

use crate::cache::{self, EXTERNAL, Kind, Span};
use crate::mem::{self, Refused};
use crate::parts::{
    Parts, error, malformed, read_numbering, read_properties, read_rels, read_styles, xml_part,
};
use crate::writer::Writer;
use crate::{MAX_TEXT, MAX_TITLE, images};

pub(crate) const INFO: &str = "info.txt";
/// The most of `info.txt` that is read.
const INFO_BYTES: usize = 4096;

fn oom(_: Refused) -> DocumentError {
    DocumentError::OutOfMemory
}

/// An open Word document.
#[derive(Debug)]
pub struct DocxDocument {
    /// The archive: `""`, the book (root 3).
    path: String,
    /// The document's folder: `""`, the cache (root 6).
    dir: String,
    metadata: Metadata,
    /// The text's length, as places are counted.
    length: u32,
    fit: Fit,
    color: bool,
}

/// What `info.txt` says: the text's length, the author, the title and the
/// language, a line each.
struct Info {
    length: u32,
    metadata: Metadata,
}

impl Info {
    /// Reads `info.txt` of the folder `dir`. A title the document has none of
    /// is left to the library (it names the book by its file).
    fn read(storage: &dyn Storage, dir: &str) -> Result<Self, DocumentError> {
        let file = cache::path(dir, INFO).map_err(oom)?;
        let size = match storage.size(&file) {
            Ok(size) => size,
            Err(paperpaw_sdk::StorageError::NotFound) => 0,
            Err(error) => return Err(error.into()),
        };
        let mut bytes =
            mem::zeroed(usize::try_from(size).unwrap_or(0).min(INFO_BYTES)).map_err(oom)?;
        let n = storage.read_at(&file, 0, &mut bytes)?;
        bytes.truncate(n);
        let info = String::from_utf8_lossy(&bytes);
        let mut lines = info.lines();
        let length = lines.next().and_then(|l| l.parse().ok()).unwrap_or(0);
        let authors = lines.next().unwrap_or("");
        let title = lines.next().unwrap_or("");
        let language = lines.next().unwrap_or("");
        let mut metadata = Metadata::default();
        if !title.is_empty() {
            metadata.title = Some(mem::string(title).map_err(oom)?);
        }
        if !authors.is_empty() {
            let mut list = Vec::new();
            mem::push(&mut list, mem::string(authors).map_err(oom)?).map_err(oom)?;
            metadata.authors = list;
        }
        if !language.is_empty() {
            metadata.language = Some(mem::string(language).map_err(oom)?);
        }
        Ok(Self { length, metadata })
    }
}

/// `text` with its words set apart by single spaces, at most `MAX_TITLE`
/// bytes.
fn one_line(text: &str) -> Result<String, Refused> {
    let mut out = String::new();
    out.try_reserve_exact(text.len().min(MAX_TITLE))
        .map_err(|_| Refused)?;
    for word in text.split_whitespace() {
        if out.len() + word.len() + 1 > MAX_TITLE {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    Ok(out)
}

impl DocxDocument {
    /// The document of the native provider whose cache is whole: the book is
    /// the archive (`""`), the cache is root 6 (`""`). A title the document
    /// has none of is left to the library.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn native(storage: &dyn Storage, fit: Fit, color: bool) -> Result<Self, DocumentError> {
        let info = Info::read(storage, "")?;
        Ok(Self {
            path: String::new(),
            dir: String::new(),
            metadata: info.metadata,
            length: info.length,
            fit,
            color,
        })
    }

    /// The document of a details open (the library's details): the book
    /// is the archive (`""`), and what its properties say of it is all there
    /// is: no cache is made or read, no block can be had (`length` 0). A title
    /// the document has none of is left to the library.
    ///
    /// # Errors
    /// `Malformed` for what is not a Word document, `OutOfMemory` where
    /// memory is short, the card's error where it fails.
    pub fn details(storage: &dyn Storage, fit: Fit, color: bool) -> Result<Self, DocumentError> {
        let archive = ZipArchive::open(storage, "").map_err(error)?;
        if archive
            .entry(storage, "word/document.xml")
            .map_err(error)?
            .is_none()
        {
            return Err(malformed("not a Word document"));
        }
        let mut parts = Parts::default();
        read_properties(storage, &archive, &mut parts)?;
        drop(archive);
        let mut metadata = Metadata::default();
        let title = one_line(&parts.title).map_err(oom)?;
        if !title.is_empty() {
            metadata.title = Some(title);
        }
        let author = one_line(&parts.author).map_err(oom)?;
        if !author.is_empty() {
            let mut list = Vec::new();
            mem::push(&mut list, author).map_err(oom)?;
            metadata.authors = list;
        }
        let language = language(&parts.language).map_err(oom)?;
        if !language.is_empty() {
            metadata.language = Some(language);
        }
        Ok(Self {
            path: String::new(),
            dir: String::new(),
            metadata,
            length: 0,
            fit,
            color,
        })
    }

    /// Makes the cache of the document: its text read a piece at a time,
    /// into the folder `dir` of `storage` (the cache itself for `""`). The
    /// files of an earlier, half made cache are the caller's to remove.
    ///
    /// # Errors
    /// `Malformed` for what is not a Word document, `OutOfMemory` where
    /// memory is short, the card's error where it fails.
    pub fn convert(storage: &dyn Storage, path: &str, dir: &str) -> Result<(), DocumentError> {
        let archive = ZipArchive::open(storage, path).map_err(error)?;
        let Some(document) = archive.entry(storage, "word/document.xml").map_err(error)? else {
            return Err(malformed("not a Word document"));
        };
        if document.size > MAX_TEXT {
            return Err(malformed("the document is too large"));
        }
        let mut parts = Parts::default();
        read_properties(storage, &archive, &mut parts)?;
        read_styles(storage, &archive, &mut parts)?;
        read_numbering(storage, &archive, &mut parts)?;
        read_rels(storage, &archive, &mut parts)?;
        let mut writer = Writer::new(storage, dir, &parts);
        xml_part(storage, &archive, &document, &mut |token| {
            writer.token(token);
            // A stopped writer drops what follows: the rest is not inflated.
            !writer.stopped()
        })?;
        writer.finish();
        if let Some(e) = writer.error() {
            return Err(e);
        }
        let length = writer.length();
        let pictures = core::mem::take(&mut writer.pictures);
        drop(writer);
        let list = join_lines(&pictures).map_err(oom)?;
        storage.write(&cache::path(dir, images::LIST).map_err(oom)?, &list)?;
        let info = mem::formatted(format_args!(
            "{length}\n{}\n{}\n{}\n",
            one_line(&parts.author).map_err(oom)?,
            one_line(&parts.title).map_err(oom)?,
            language(&parts.language).map_err(oom)?,
        ))
        .map_err(oom)?;
        storage.write(&cache::path(dir, INFO).map_err(oom)?, info.as_bytes())?;
        Ok(())
    }

    /// The block at `at`, as the cache has it.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn try_block_at(
        &self,
        storage: &dyn Storage,
        at: Locator,
    ) -> Result<Option<Block>, DocumentError> {
        if at.section != 0 {
            return Ok(None);
        }
        let Some(loaded) = cache::block_at(storage, &self.dir, at.offset)? else {
            return Ok(None);
        };
        let kind = match loaded.kind {
            Kind::Paragraph => BlockKind::Paragraph,
            Kind::Heading(level) => BlockKind::Heading(level),
            Kind::Image => BlockKind::Image,
        };
        let picture = match loaded.image {
            Some((index, _, _)) => Some(images::picture(
                storage, &self.path, &self.dir, index, self.fit, self.color,
            )?),
            None => None,
        };
        let (kind, text) = match (&picture, loaded.kind) {
            // A picture that cannot be shown shows what it says it is.
            (Some(None), _) => (
                BlockKind::Paragraph,
                if loaded.text.is_empty() {
                    mem::string("[…]").map_err(oom)?
                } else {
                    mem::formatted(format_args!("[{}]", loaded.text)).map_err(oom)?
                },
            ),
            _ => (kind, loaded.text),
        };
        let mut block = Block::new(kind, loaded.start, loaded.end, text)
            .continues(loaded.continues)
            .continued(loaded.continued)
            .map(loaded.map)
            .runs(loaded.runs)
            .align(loaded.align);
        if let Some(Some(image)) = picture {
            block = block.image(image);
        }
        Ok(Some(block))
    }

    /// How many entries the table of contents has.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn try_toc_len(&self, storage: &dyn Storage) -> Result<u32, DocumentError> {
        cache::toc_len(storage, &self.dir)
    }

    /// Entry `index` of the table of contents.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn try_toc_entry(
        &self,
        storage: &dyn Storage,
        index: u32,
    ) -> Result<Option<TocEntry>, DocumentError> {
        Ok(
            cache::toc_entry(storage, &self.dir, index)?.map(|(at, level, title)| TocEntry {
                title,
                level,
                at: Locator::new(0, at),
            }),
        )
    }

    /// The links that overlap the offsets `from..to` (at most 256).
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn link_spans(
        &self,
        storage: &dyn Storage,
        from: u32,
        to: u32,
    ) -> Result<Vec<Span>, DocumentError> {
        cache::link_spans(storage, &self.dir, from, to)
    }

    /// The address a link of the cache leads to: a web address of the
    /// document, or `None` for a place in it (`#<number>`).
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn address(
        &self,
        storage: &dyn Storage,
        value: u32,
    ) -> Result<Option<String>, DocumentError> {
        if value & EXTERNAL == 0 {
            return Ok(None);
        }
        cache::url_at(storage, &self.dir, value & !EXTERNAL)
    }

    /// Where the bookmark of `href` (`#<number>`) is.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn try_link_target(
        &self,
        storage: &dyn Storage,
        href: &str,
    ) -> Result<Option<Locator>, DocumentError> {
        let Some(hash) = href.strip_prefix('#').and_then(|h| h.parse::<u32>().ok()) else {
            return Ok(None);
        };
        Ok(cache::mark(storage, &self.dir, hash)?.map(|at| Locator::new(0, at)))
    }

    /// The picture `index` decoded, at the page's size or, for a larger
    /// `side`, at that (a zoom); `None` for one that cannot be shown.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn picture(
        &self,
        storage: &dyn Storage,
        index: u32,
        side: u32,
    ) -> Result<Option<ImageRef>, DocumentError> {
        images::shown_at(
            storage, &self.path, &self.dir, index, side, self.fit, self.color,
        )
    }

    /// Whether the picture of `image` is decoded.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub fn is_decoded(
        &self,
        storage: &dyn Storage,
        image: &ImageRef,
    ) -> Result<bool, DocumentError> {
        images::is_decoded(storage, &image.path, self.color)
    }
}

/// The places of the pictures, a line each.
fn join_lines(lines: &[String]) -> Result<Vec<u8>, Refused> {
    let mut out = Vec::new();
    out.try_reserve_exact(lines.iter().map(|l| l.len() + 1).sum())
        .map_err(|_| Refused)?;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(line.as_bytes());
    }
    Ok(out)
}

/// The language as a tag: letters, digits and `-`, at most 32.
fn language(text: &str) -> Result<String, Refused> {
    let mut out = String::new();
    out.try_reserve_exact(text.trim().len().min(32))
        .map_err(|_| Refused)?;
    for c in text
        .trim()
        .chars()
        .take(32)
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
    {
        out.push(c);
    }
    Ok(out)
}

impl Document for DocxDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, section: u32) -> u32 {
        if section == 0 { self.length } else { 0 }
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        self.try_block_at(storage, at)
    }

    fn toc_len(&self, storage: &dyn Storage) -> u32 {
        self.try_toc_len(storage).unwrap_or(0)
    }

    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        self.try_toc_entry(storage, index).ok().flatten()
    }

    fn prepare_image(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        images::prepare(storage, &self.dir, image, self.fit, self.color)
            .ok()
            .flatten()
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        if section != 0 {
            return Vec::new();
        }
        let Ok(spans) = self.link_spans(storage, from, to) else {
            return Vec::new();
        };
        let mut links = Vec::new();
        for (start, end, value) in spans {
            let href = if value & EXTERNAL != 0 {
                match self.address(storage, value) {
                    Ok(Some(url)) => url,
                    _ => continue,
                }
            } else {
                format!("#{value}")
            };
            links.push(Link::new(start, end, href));
        }
        links
    }

    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        self.try_link_target(storage, href).ok().flatten()
    }
}
