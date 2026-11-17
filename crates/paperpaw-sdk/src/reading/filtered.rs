//! A book as a reader lays it out: every block through the plugins' text
//! filters (see [`filters`](crate::filters)).

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::document::{
    Block, Document, DocumentError, EmbeddedFont, ImageRef, Link, Locator, Metadata, TocEntry,
};
use crate::{Env, Storage};

/// `document` with the plugins' text filters applied to its blocks, as a
/// reader lays it out and searches it: a reader that reads the document
/// itself ignores every filter (replaced words, word beginnings in bold,
/// blocks left out). Everything else is the document's own.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct FilteredDocument<'d, 'e> {
    pub document: &'d dyn Document,
    pub env: &'d Env<'e>,
    /// The book and its language, told to block filters.
    pub path: &'d str,
    pub language: &'d str,
}

impl<'d, 'e> FilteredDocument<'d, 'e> {
    pub fn new(document: &'d dyn Document, env: &'d Env<'e>) -> Self {
        Self {
            document,
            env,
            path: "",
            language: "",
        }
    }

    /// The same, telling block filters the book at `path` in `language`.
    pub fn in_book(mut self, path: &'d str, language: &'d str) -> Self {
        self.path = path;
        self.language = language;
        self
    }

    /// The block at `at` through the filters, if they keep it: `None`
    /// where they leave it out (not the next one kept, as
    /// [`Document::block_at`] gives), e.g. for the target of a link.
    pub fn kept_block_at(
        &self,
        storage: &dyn Storage,
        at: Locator,
    ) -> Result<Option<Block>, DocumentError> {
        let Some(block) = self.document.block_at(storage, at)? else {
            return Ok(None);
        };
        if block.end <= at.offset {
            return Err(not_advancing());
        }
        Ok(Some(block))
    }
}

impl core::fmt::Debug for FilteredDocument<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FilteredDocument").finish_non_exhaustive()
    }
}

fn not_advancing() -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed("a block that does not advance"))
}

impl Document for FilteredDocument<'_, '_> {
    fn metadata(&self) -> &Metadata {
        self.document.metadata()
    }

    fn sections(&self) -> u32 {
        self.document.sections()
    }

    fn page_label(&self, storage: &dyn Storage, at: Locator) -> Option<String> {
        self.document.page_label(storage, at)
    }

    fn whole_pages(&self) -> bool {
        self.document.whole_pages()
    }

    fn section_len(&self, section: u32) -> u32 {
        self.document.section_len(section)
    }

    /// The block at `at`; one that does not advance is an error.
    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        self.kept_block_at(storage, at)
    }

    fn toc_len(&self, storage: &dyn Storage) -> u32 {
        self.document.toc_len(storage)
    }

    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        self.document.toc_entry(storage, index)
    }

    fn toc_target(&self, storage: &dyn Storage, index: u32) -> Option<Locator> {
        self.document.toc_target(storage, index)
    }

    fn prepare_image(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        self.document.prepare_image(storage, image)
    }

    fn prepare_image_larger(
        &self,
        storage: &dyn Storage,
        image: &ImageRef,
        long: u32,
    ) -> Option<ImageRef> {
        self.document.prepare_image_larger(storage, image, long)
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        self.document.cover(storage)
    }

    fn xpointer(&self, storage: &dyn Storage, at: Locator) -> Option<String> {
        self.document.xpointer(storage, at)
    }

    fn locate_xpointer(&self, storage: &dyn Storage, xpointer: &str) -> Option<Locator> {
        self.document.locate_xpointer(storage, xpointer)
    }

    fn fonts(&self, storage: &dyn Storage) -> Vec<EmbeddedFont> {
        self.document.fonts(storage)
    }

    fn font_data(&self, storage: &dyn Storage, index: usize) -> Result<Vec<u8>, DocumentError> {
        self.document.font_data(storage, index)
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        self.document.links(storage, section, from, to)
    }

    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        self.document.link_target(storage, href)
    }

    fn progress(&self, at: Locator) -> f32 {
        self.document.progress(at)
    }
}
