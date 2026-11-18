//! A picture and those after it in its folder as a [`Document`]: a page
//! each, in the order of their names, a picture decoded when its page is
//! first shown and kept in the cache.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{
    Align, Block, BlockKind, Document, DocumentError, ImageRef, Locator, Metadata, OffsetMap,
};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::{OpenOptions, Storage};

use crate::names::{Page, copy, is_picture, pages_from};
use crate::pictures;

/// An open picture, as a book of the pictures from it on.
#[derive(Debug)]
pub struct ImagesDocument {
    /// The pages: the file's name and size.
    pages: Vec<Page>,
    metadata: Metadata,
    color: bool,
    image_fit: Fit,
    cover_fit: Fit,
}

fn malformed(why: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(why))
}

impl ImagesDocument {
    /// Opens the picture `name` of the book's folder (root 3) as the first
    /// page, its decoded pictures in the document cache (root 6, reached by
    /// the paths that start with [`pictures::CACHE`]): the native provider's
    /// form.
    pub fn open_native(
        storage: &dyn Storage,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Self, DocumentError> {
        if name.contains('/') || name.contains(':') {
            return Err(malformed("not a name in the book's folder"));
        }
        if !is_picture(name) {
            return Err(malformed("not a picture"));
        }
        let first = (copy(name)?, storage.size(name)?);
        let pages = pages_from(storage, "", &first)?;
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let metadata = Metadata::default().title(copy(stem)?);
        Ok(Self {
            pages,
            metadata,
            color: options.color,
            image_fit: options.image_fit,
            cover_fit: options.cover_fit,
        })
    }

    /// Page `index` as shown in its page, decoded now if it is not yet.
    pub(crate) fn page_picture(
        &self,
        storage: &dyn Storage,
        index: u32,
        side: u32,
    ) -> Result<Option<ImageRef>, DocumentError> {
        let fit = if side == 0 {
            self.image_fit
        } else {
            Fit::side(side)
        };
        let index = index as usize;
        let Some((name, size)) = self.pages.get(index) else {
            return Ok(None);
        };
        let base = pictures::base(pictures::CACHE, index, name, *size)?;
        self.picture(storage, index, &base, fit)
    }

    /// Page `index` decoded into the picture `base`, if it can be.
    fn picture(
        &self,
        storage: &dyn Storage,
        index: usize,
        base: &str,
        fit: Fit,
    ) -> Result<Option<ImageRef>, DocumentError> {
        let Some((name, size)) = self.pages.get(index) else {
            return Ok(None);
        };
        pictures::shown(storage, name, *size, base, fit, self.color)
    }
}

impl Document for ImagesDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn whole_pages(&self) -> bool {
        true
    }

    fn sections(&self) -> u32 {
        self.pages.len() as u32
    }

    fn section_len(&self, _: u32) -> u32 {
        1
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let index = at.section as usize;
        let Some((name, size)) = self.pages.get(index) else {
            return Ok(None);
        };
        if at.offset >= 1 {
            return Ok(None);
        }
        let base = pictures::base(pictures::CACHE, index, name, *size)?;
        let image = self.picture(storage, index, &base, self.image_fit)?;
        // A picture that cannot be shown is named instead.
        let (kind, text) = match &image {
            Some(_) => (BlockKind::Image, String::new()),
            None => (BlockKind::Paragraph, copy(name)?),
        };
        Ok(Some(
            Block::new(kind, 0, 1, text)
                .map(OffsetMap::IDENTITY)
                .align(Align::Center)
                .image(image),
        ))
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        self.cover_at(storage, self.cover_fit)
    }
}

impl ImagesDocument {
    /// The cover no larger than `fit` (the side the opener asked for).
    pub(crate) fn cover_at(
        &self,
        storage: &dyn Storage,
        fit: Fit,
    ) -> Result<Option<ImageRef>, DocumentError> {
        let mut digits = [0u8; 20];
        let long = pictures::decimal(u64::from(fit.long), &mut digits);
        let base = pictures::join(pictures::CACHE, &["cover-", long])?;
        self.picture(storage, 0, &base, fit)
    }
}
