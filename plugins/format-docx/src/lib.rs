//! Word documents (`.docx`) for PaperPaw.
//!
//! - **What is read:** the text of `word/document.xml` with its paragraphs,
//!   headings (by the style's name, `Title`, or the outline level),
//!   bulleted and numbered lists (nested), tables (a row a line, cells set
//!   apart by `|`), line breaks, bold, italic, underline, strike-through,
//!   superscript and subscript, centred and right-aligned text, links (web
//!   addresses and places in the document) and pictures (JPEG, PNG, GIF);
//!   title, author and language from the document's properties; a
//!   table of contents made of the headings.
//! - **Left out:** footnotes and endnotes, headers and footers, comments,
//!   deleted text of tracked changes, fields' codes, text boxes' place.
//! - **How:** the archive is read in pieces and the blocks of the text
//!   written in pieces, once, into a cache in the plugin's folder for the
//!   document; a document of any length is converted in a few kilobytes of
//!   memory, and what a hostile archive holds is bounded (sizes, counts,
//!   nesting). Pictures are taken out of the archive and decoded when their
//!   page shows.
//!
//! Nothing here is in the interface: it is a format plugin like the
//! others, on the SDK only.

#![no_std]

extern crate alloc;

mod cache;
mod document;
mod images;
mod mem;
mod parts;
pub mod provider;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
/// The interface-7 plugin.
pub mod native_plugin;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
mod writer;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec;

use paperpaw_sdk::{Env, FormatSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub use document::DocxDocument;

pub const PLUGIN: Id = Id::new_static("format-docx");
pub const FORMAT: Id = Id::new_static("format-docx.docx");

/// The Word documents format plugin.
#[derive(Debug, Default)]
pub struct DocxFormat;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// The most the text of a document may be, unpacked, in bytes.
pub(crate) const MAX_TEXT: u32 = 64 << 20;
/// The most the text of the blocks may be, in bytes: more is left off.
pub(crate) const MAX_TEXT_BYTES: u32 = 48 << 20;
/// The most a part with styles, lists, links or properties may be.
pub(crate) const MAX_SMALL: u32 = 4 << 20;
/// Pictures shown at most, and the largest one's size in bytes.
pub(crate) const MAX_IMAGES: usize = 100;
pub(crate) const MAX_IMAGE: u32 = 8 << 20;
/// Entries of the small tables kept at most: styles, lists, links; and the
/// bytes of the texts of the links kept.
pub(crate) const MAX_STYLES: usize = 256;
pub(crate) const MAX_LISTS: usize = 256;
pub(crate) const MAX_RELS: usize = 512;
pub(crate) const MAX_REL_BYTES: usize = 32 * 1024;
/// Longest address, name and alternative text kept; a picture's place in
/// the archive.
pub(crate) const MAX_ADDRESS: usize = 1024;
pub(crate) const MAX_PATH: usize = 256;
pub(crate) const MAX_NAME: usize = 100;
pub(crate) const MAX_ALT: usize = 300;
/// Longest title and author kept.
pub(crate) const MAX_TITLE: usize = 300;
/// Lists nested deeper are read as at this depth.
pub(crate) const MAX_DEPTH: usize = 4;
/// Bookmarks waiting for the next paragraph, at most.
pub(crate) const MAX_PENDING: usize = 16;
/// Headings listed in the contents at most, and the longest title.
pub(crate) const MAX_TOC: usize = 500;
pub(crate) const MAX_TOC_TITLE: usize = 100;

// The formats of a run.
pub(crate) const BOLD: u8 = 1;
pub(crate) const ITALIC: u8 = 2;
pub(crate) const UNDER: u8 = 4;
pub(crate) const STRIKE: u8 = 8;
pub(crate) const SUP: u8 = 16;
pub(crate) const SUB: u8 = 32;

impl Plugin for DocxFormat {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "DOCX documents", paperpaw_sdk::package_version!())
            .description("Opens .docx files, as saved by Word and other programs.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each document, moved and removed with it.
        r.add_book_cache(Env::plugin_dir(PLUGIN.as_str()), "")?;
        r.add_format(FormatSpec::new(
            FORMAT,
            "format-docx.title",
            vec![Cow::Borrowed("docx")],
            Box::new(document::opener),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
