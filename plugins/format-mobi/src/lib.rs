//! Mobipocket books (`.mobi`, `.azw`, `.azw3`) without DRM, for PaperPaw.
//!
//! - **What is read:** the Palm database a book is, its PalmDOC and MOBI
//!   headers and EXTH records (title, authors, language, subjects,
//!   description, cover), the text with PalmDOC compression or none (UTF-8
//!   or Windows-1252), its markup (paragraphs, headings, bold and italic,
//!   line breaks, links by position, pictures), and a table of contents
//!   made of its headings.
//! - **What is refused, with a message:** books with DRM, books with
//!   Huffman compression (HUFF/CDIC), and KF8 books (AZW3).
//! - **How:** the book is converted once, a text record at a time (one
//!   record of 4 KB is in memory), into the document cache of blocks the host
//!   keeps for the book; reading finds a block by halving an index.
//!   Pictures are decoded when their page shows, a few rows at a time.
//!
//! Interface 7 only: the plugin declares its format and answers the host's
//! `core.format` calls ([`provider`]).

#![no_std]

extern crate alloc;

mod cache;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod document;
mod header;
mod images;
mod mem;
mod native_entry;
/// The interface-7 plugin.
pub mod native_plugin;
mod palmdoc;
pub mod provider;
mod text;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub use document::MobiDocument;

pub const PLUGIN: Id = Id::new_static("format-mobi");

/// Whether the first bytes of a file are a Mobipocket book's.
pub(crate) fn is_mobi(head: &[u8]) -> bool {
    head.get(60..68) == Some(b"BOOKMOBI")
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
