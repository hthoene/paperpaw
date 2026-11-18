//! A Mobipocket book as a [`Document`]: converted once into the cache (see
//! [`crate::cache`]), read from there.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::document::{
    Block, BlockKind, Document, DocumentError, ImageRef, Link, Locator, Metadata, TocEntry,
};
use paperpaw_sdk::image::{Fit, ImageFile};
use paperpaw_sdk::{OpenOptions, Storage};

use crate::cache::{self, Kind, Output};
use crate::header::{Header, record_range};
use crate::images::{self, Book};
use crate::mem::Refused;
use crate::palmdoc;
use crate::text::Converter;

/// An open Mobipocket book.
#[derive(Debug)]
pub struct MobiDocument {
    path: String,
    header: Header,
    size: u64,
    /// The book's folder (empty where the cache is the document cache of the
    /// interface-7 provider).
    dir: String,
    metadata: Metadata,
    fit: Fit,
    cover_fit: Fit,
    color: bool,
}

fn malformed(why: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(why))
}

/// The making of a book's cache, one text record at a time: each step reads
/// a record, converts it and puts the blocks it made on the card, so that a
/// caller with a time to keep can stop between two.
pub(crate) struct Conversion {
    header: Header,
    size: u64,
    fit: Fit,
    converter: Converter,
    /// The next text record, counted from 1.
    next: u32,
    fed: usize,
    raw: Vec<u8>,
    text: Vec<u8>,
}

impl Conversion {
    pub(crate) fn new(header: Header, size: u64, fit: Fit) -> Self {
        let converter = Converter::new(header.encoding, Output::default());
        Self {
            header,
            size,
            fit,
            converter,
            next: 1,
            fed: 0,
            raw: Vec::new(),
            text: Vec::new(),
        }
    }

    /// Starts again from the first record: what was made is dropped.
    pub(crate) fn restart(&mut self) {
        self.converter = Converter::new(self.header.encoding, Output::default());
        self.next = 1;
        self.fed = 0;
        self.raw.clear();
        self.text.clear();
    }

    /// How much of the text is converted, in millionths.
    pub(crate) fn progress_ppm(&self) -> u32 {
        let total = u64::from(self.header.text_length.max(1));
        u32::try_from((self.fed as u64).saturating_mul(1_000_000) / total)
            .unwrap_or(1_000_000)
            .min(1_000_000)
    }

    /// Converts the next text record of the book at `path`, its cache in the
    /// folder `dir`; `true` when the last one is done and the cache is whole.
    ///
    /// # Errors
    /// What the book or the card or memory says; after an error the
    /// conversion is to be started again ([`Conversion::restart`]).
    pub(crate) fn step(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        dir: &str,
    ) -> Result<bool, DocumentError> {
        let header = &self.header;
        if self.next > header.text_records {
            return Ok(true);
        }
        let mut pictures = Book {
            storage,
            path,
            header,
            size: self.size,
            fit: self.fit,
        };
        let record_size = header.record_size as usize;
        let Some((start, end)) = record_range(storage, path, header.records, self.size, self.next)?
        else {
            return Err(malformed("the text is cut off"));
        };
        let want = (end - start).min(2 * record_size as u64 + 64) as usize;
        self.raw.clear();
        self.raw.try_reserve(want).map_err(|_| Refused)?;
        self.raw.resize(want, 0);
        let n = storage.read_at(path, start, &mut self.raw)?;
        self.raw.truncate(n);
        let len = palmdoc::text_len(&self.raw, header.extra_flags);
        self.text.clear();
        self.text.try_reserve(record_size).map_err(|_| Refused)?;
        if header.compressed {
            // A faulty record keeps the text it gave before the fault.
            palmdoc::decompress(&self.raw[..len], &mut self.text, record_size).ok();
        } else {
            self.text
                .extend_from_slice(&self.raw[..len.min(record_size)]);
        }
        let last = self.next == header.text_records;
        let expected = if last {
            (header.text_length as usize).saturating_sub(self.fed)
        } else {
            record_size
        };
        self.text.truncate(expected);
        // Offsets are those of the text as it should be: a record that
        // is short is made up with spaces.
        if !last && self.text.len() < expected {
            self.text.resize(expected, b' ');
        }
        self.fed += self.text.len();
        self.converter.feed(&self.text, &mut pictures);
        self.converter.out.flush(storage, dir, false);
        if let Some(error) = self.converter.out.error() {
            return Err(error);
        }
        self.next += 1;
        if self.next <= header.text_records {
            return Ok(false);
        }
        self.converter.finish();
        self.converter.out.flush(storage, dir, true);
        match self.converter.out.error() {
            Some(error) => Err(error),
            None => Ok(true),
        }
    }

    /// The book, once its cache is whole: the folder of the cache is `dir`.
    pub(crate) fn into_document(
        self,
        path: String,
        dir: String,
        cover_fit: Fit,
        color: bool,
    ) -> MobiDocument {
        let mut header = self.header;
        let metadata = core::mem::take(&mut header.metadata);
        MobiDocument {
            path,
            header,
            size: self.size,
            dir,
            metadata,
            fit: self.fit,
            cover_fit,
            color,
        }
    }
}

impl MobiDocument {
    /// Opens the book at `path`, converting it whole into the folder `dir`
    /// first (the host's `prepare` calls make the same cache in steps, a
    /// record each; the host owns the stamp that says it is whole).
    ///
    /// # Errors
    /// What the book, the card or memory says.
    pub fn open_converted(
        storage: &dyn Storage,
        path: &str,
        dir: &str,
        options: &OpenOptions,
    ) -> Result<Self, DocumentError> {
        let header = Header::read(storage, path)?;
        let size = storage.size(path)?;
        Self::convert(storage, path, &header, size, dir, options)?;
        let mut metadata = header.metadata.clone();
        if metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(name.rsplit_once('.').map_or(name, |(s, _)| s).to_string());
        }
        Ok(Self {
            path: path.to_string(),
            header,
            size,
            dir: dir.to_string(),
            metadata,
            fit: options.image_fit,
            cover_fit: options.cover_fit,
            color: options.color,
        })
    }

    /// Makes the cache of the book: its text read a record at a time.
    fn convert(
        storage: &dyn Storage,
        path: &str,
        header: &Header,
        size: u64,
        dir: &str,
        options: &OpenOptions,
    ) -> Result<(), DocumentError> {
        storage.remove_dir_all(dir).ok();
        storage.create_dir(dir)?;
        let mut conversion = Conversion::new(header.clone(), size, options.image_fit);
        while !conversion.step(storage, path, dir)? {}
        Ok(())
    }

    fn book<'a>(&'a self, storage: &'a dyn Storage) -> Book<'a> {
        Book {
            storage,
            path: &self.path,
            header: &self.header,
            size: self.size,
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

    /// The links over the source offsets `from..to`: where each starts and
    /// ends and the position it leads to.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub(crate) fn link_spans(
        &self,
        storage: &dyn Storage,
        from: u32,
        to: u32,
    ) -> Result<Vec<(u32, u32, u32)>, DocumentError> {
        cache::link_spans(storage, &self.dir, from, to)
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
        let Some(record) = book.picture_record(index) else {
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
        let decoded = book.decode(record, &to, fit, self.color)?;
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

    /// The cover, decoded to `fit`.
    ///
    /// # Errors
    /// A card or memory that fails.
    pub(crate) fn cover_in(
        &self,
        storage: &dyn Storage,
        fit: Fit,
    ) -> Result<Option<ImageRef>, DocumentError> {
        let Some(record) = self.header.cover else {
            return Ok(None);
        };
        let to = images::cover_file(&self.dir, fit.long, self.color)?;
        if let Some(file) = ImageFile::open(storage, &to) {
            return Ok(Some(ImageRef {
                path: file.path,
                width: file.width,
                height: file.height,
            }));
        }
        self.book(storage).decode(record, &to, fit, self.color)
    }
}

impl Document for MobiDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, section: u32) -> u32 {
        if section == 0 {
            self.header.text_length
        } else {
            0
        }
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

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        self.cover_in(storage, self.cover_fit)
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        if section != 0 {
            return Vec::new();
        }
        cache::links(storage, &self.dir, from, to)
    }

    fn link_target(&self, _: &dyn Storage, href: &str) -> Option<Locator> {
        let position: u32 = href.strip_prefix('#')?.parse().ok()?;
        (position < self.header.text_length).then(|| Locator::new(0, position))
    }
}
