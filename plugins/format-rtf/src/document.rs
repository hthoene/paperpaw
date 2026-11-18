//! An RTF document as a [`Document`]: converted once into the cache (see
//! [`crate::cache`]), read from there.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::document::{
    Block, BlockKind, Document, DocumentError, ImageRef, Locator, Metadata, TocEntry,
};
use paperpaw_sdk::image::{Fit, ImageFile};
use paperpaw_sdk::{OpenOptions, Storage};

use crate::cache::{self, Kind, Output};
use crate::header::Header;
use crate::images::{self, Book};
use crate::mem::Refused;
use crate::rtf::Converter;

/// Bytes of the document read and converted by one step.
const STEP: u64 = 8 * 1024;

/// An open RTF document.
#[derive(Debug)]
pub struct RtfDocument {
    path: String,
    size: u32,
    /// The document's cache folder (empty where the cache is the document
    /// cache of the interface-7 provider).
    dir: String,
    metadata: Metadata,
    fit: Fit,
    color: bool,
}

/// The making of a document's cache, a piece of the file at a time: each
/// step reads a few kilobytes, converts them and puts the blocks it made on
/// the card, so that a caller with a time to keep can stop between two.
pub(crate) struct Conversion {
    header: Header,
    fit: Fit,
    converter: Converter,
    /// The next byte of the file to read.
    at: u64,
    raw: Vec<u8>,
}

impl Conversion {
    pub(crate) fn new(header: Header, fit: Fit) -> Self {
        let converter = Converter::new(header.charset, Output::default());
        Self {
            header,
            fit,
            converter,
            at: 0,
            raw: Vec::new(),
        }
    }

    /// Starts again from the first byte: what was made is dropped.
    pub(crate) fn restart(&mut self) {
        self.converter = Converter::new(self.header.charset, Output::default());
        self.at = 0;
        self.raw.clear();
    }

    /// How much of the file is converted, in millionths.
    pub(crate) fn progress_ppm(&self) -> u32 {
        let total = u64::from(self.header.size).max(1);
        u32::try_from(self.at.saturating_mul(1_000_000) / total)
            .unwrap_or(1_000_000)
            .min(1_000_000)
    }

    /// Converts the next piece of the document at `path`, its cache in the
    /// folder `dir`; `true` when the last one is done and the cache is whole.
    ///
    /// # Errors
    /// What the document or the card or memory says; after an error the
    /// conversion is to be started again ([`Conversion::restart`]).
    pub(crate) fn step(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        dir: &str,
    ) -> Result<bool, DocumentError> {
        let size = u64::from(self.header.size);
        let want = size.saturating_sub(self.at).min(STEP) as usize;
        let mut read = 0;
        if want > 0 {
            self.raw.clear();
            self.raw.try_reserve(want).map_err(|_| Refused)?;
            self.raw.resize(want, 0);
            read = storage.read_at(path, self.at, &mut self.raw)?;
            self.raw.truncate(read);
            let mut pictures = Book {
                storage,
                path,
                fit: self.fit,
            };
            self.converter.feed(&self.raw, &mut pictures);
            self.at += read as u64;
            self.converter.out.flush(storage, dir, false);
            if let Some(error) = self.converter.out.error() {
                return Err(error);
            }
        }
        // A file that ends before its size said is done where it ends.
        if read > 0 && self.at < size {
            return Ok(false);
        }
        self.converter.finish();
        self.converter.out.flush(storage, dir, true);
        match self.converter.out.error() {
            Some(error) => Err(error),
            None => Ok(true),
        }
    }

    /// The document, once its cache is whole: the folder of the cache is `dir`.
    pub(crate) fn into_document(self, path: String, dir: String, color: bool) -> RtfDocument {
        let mut header = self.header;
        let metadata = core::mem::take(&mut header.metadata);
        RtfDocument {
            path,
            size: header.size,
            dir,
            metadata,
            fit: self.fit,
            color,
        }
    }
}

impl RtfDocument {
    /// Opens the document at `path`, converting it whole into the folder
    /// `dir` first (the host's `prepare` calls make the same cache in steps;
    /// the host owns the stamp that says it is whole).
    ///
    /// # Errors
    /// What the document, the card or memory says.
    pub fn open_converted(
        storage: &dyn Storage,
        path: &str,
        dir: &str,
        options: &OpenOptions,
    ) -> Result<Self, DocumentError> {
        let header = Header::read(storage, path)?;
        storage.remove_dir_all(dir).ok();
        storage.create_dir(dir)?;
        let mut conversion = Conversion::new(header.clone(), options.image_fit);
        while !conversion.step(storage, path, dir)? {}
        let mut metadata = header.metadata.clone();
        if metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(name.rsplit_once('.').map_or(name, |(s, _)| s).to_string());
        }
        Ok(Self {
            path: path.to_string(),
            size: header.size,
            dir: dir.to_string(),
            metadata,
            fit: options.image_fit,
            color: options.color,
        })
    }

    fn book<'a>(&'a self, storage: &'a dyn Storage) -> Book<'a> {
        Book {
            storage,
            path: &self.path,
            fit: self.fit,
        }
    }

    /// Table of contents entry `index`, where memory and the card say so.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub(crate) fn try_toc_entry(
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

    /// How many entries the table of contents has.
    ///
    /// # Errors
    /// A card that fails.
    pub(crate) fn try_toc_len(&self, storage: &dyn Storage) -> Result<u32, DocumentError> {
        cache::toc_len(storage, &self.dir)
    }

    /// Picture `index` (counted from the first picture) decoded: to its
    /// page's size, or at `side` pixels where that is larger. `None` for a
    /// picture that is none or cannot be decoded (it is not tried twice).
    ///
    /// # Errors
    /// A card or memory that fails.
    pub(crate) fn picture(
        &self,
        storage: &dyn Storage,
        index: u32,
        side: u32,
    ) -> Result<Option<ImageRef>, DocumentError> {
        let book = self.book(storage);
        let Some(range) = cache::picture_range(storage, &self.dir, index)? else {
            return Ok(None);
        };
        let (to, fit) = if side > self.fit.long {
            (
                images::zoomed(&self.dir, index, side, self.color)?,
                Fit::side(side),
            )
        } else {
            (images::file(&self.dir, index, self.color)?, self.fit)
        };
        if let Some(file) = ImageFile::open(storage, &to) {
            return Ok(Some(ImageRef {
                path: file.path,
                width: file.width,
                height: file.height,
            }));
        }
        let failed = images::failed(&self.dir, index)?;
        if storage.exists(&failed) {
            return Ok(None);
        }
        let decoded = book.decode(range, &to, fit, self.color)?;
        if decoded.is_none() {
            storage.write(&failed, b"").ok();
        }
        Ok(decoded)
    }

    /// Where picture `index` is (or would be) decoded to its page's size.
    ///
    /// # Errors
    /// Memory that is short.
    pub(crate) fn picture_file(&self, index: u32) -> Result<String, Refused> {
        images::file(&self.dir, index, self.color)
    }
}

impl Document for RtfDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, section: u32) -> u32 {
        if section == 0 { self.size } else { 0 }
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
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
        let mut block = Block::new(kind, loaded.start, loaded.end, loaded.text)
            .continues(loaded.continues)
            .continued(loaded.continued)
            .map(loaded.map)
            .runs(loaded.runs)
            .align(loaded.align);
        if let Some((index, width, height)) = loaded.image {
            block = block.image(ImageRef {
                path: self.picture_file(index)?,
                width,
                height,
            });
        }
        Ok(Some(block))
    }

    fn toc_len(&self, storage: &dyn Storage) -> u32 {
        self.try_toc_len(storage).unwrap_or(0)
    }

    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        self.try_toc_entry(storage, index).ok().flatten()
    }

    fn prepare_image(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        if ImageFile::open(storage, &image.path).is_some() {
            return None;
        }
        let index = images::index_of(&image.path)?;
        self.picture(storage, index, 0).ok().flatten()
    }
}
