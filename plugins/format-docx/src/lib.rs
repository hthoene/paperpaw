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
//!   written in pieces, once, into the cache the host gives the document;
//!   a document of any length is converted in a few kilobytes of memory,
//!   and what a hostile archive holds is bounded (sizes, counts, nesting).
//!   Pictures are taken out of the archive and decoded when the host asks.
//!
//! The plugin is an interface-7 `core.format` provider only ([`provider`],
//! [`native_plugin`]); the interface-6 form is gone.

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

pub use document::DocxDocument;

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
