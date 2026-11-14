//! Plain text (`.txt`) support for PaperPaw.
//!
//! - Encodings: UTF-8 (with or without BOM), where stray bytes are read as
//!   Windows-1252; UTF-16 with a BOM, read from a copy in UTF-8 made when
//!   the file is first opened; otherwise Windows-1252.
//! - Paragraphs: separated by blank lines when the text uses them (typical
//!   for hard-wrapped e-texts), otherwise one paragraph per line.
//! - Project Gutenberg files: the licence header and footer are skipped and
//!   title, author and language are read from the header.
//!
//! The file is never loaded completely; each request reads a few kilobytes
//! around the position (what the request before it read is used again),
//! so arbitrarily large files work on small devices.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod document;
mod native_entry;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod preparation;
pub mod provider;
mod utf16;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub use document::TxtDocument;

pub const PLUGIN: Id = Id::new_static("format-txt");
pub const FORMAT: Id = Id::new_static("format-txt.txt");

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
