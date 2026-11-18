//! RTF documents (`.rtf`) for PaperPaw.
//!
//! - **What is read:** paragraphs, headings (the outline level, or a style
//!   named `heading N` or `Title`), bold, italic, underline, strike-through,
//!   super- and subscript, alignment, bulleted and numbered lists, tables (a
//!   row a line, the cells set apart by `|`), line breaks, `\uN` characters
//!   and `\'hh` bytes of the document's code page (Windows-1252, 1250 and
//!   1251; any other reads as 1252), embedded JPEG and PNG pictures; the
//!   title, author, subject and language of the document's information; a
//!   table of contents made of its headings.
//! - **Left out:** headers, footers, footnotes, fields' codes, hidden text,
//!   objects, pictures of other kinds (metafiles, bitmaps), every
//!   destination that is not known; links and covers.
//! - **How:** the file is read in pieces of 8 KB by a state machine that
//!   keeps a stack of groups, and the blocks of the text are written in
//!   pieces, once, into the document cache the host keeps; a document of
//!   any length is converted in a few tens of kilobytes of memory, and
//!   what a hostile file holds is bounded (size, nesting, names, pictures).
//!   Pictures stay where they are in the file as hexadecimal text and are
//!   decoded, a few rows at a time, when their page shows.
//!
//! Interface 7 only: the plugin declares its format and answers the host's
//! `core.format` calls ([`provider`]).

#![no_std]

extern crate alloc;

mod cache;
mod charset;
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
pub mod provider;
mod rtf;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub use document::RtfDocument;

pub const PLUGIN: Id = Id::new_static("format-rtf");

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
