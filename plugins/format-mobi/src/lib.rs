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
//!   record of 4 KB is in memory), into a cache of blocks in the plugin's
//!   folder for the book; reading finds a block by halving an index.
//!   Pictures are decoded when their page shows, a few rows at a time.
//!
//! Nothing here is in the interface: it is a format plugin like the
//! others, on the SDK only.

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

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec;

use paperpaw_sdk::{Env, FormatSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub use document::MobiDocument;

pub const PLUGIN: Id = Id::new_static("format-mobi");
pub const FORMAT: Id = Id::new_static("format-mobi.mobi");

/// The Mobipocket format plugin.
#[derive(Debug, Default)]
pub struct MobiFormat;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// Whether the first bytes of a file are a Mobipocket book's.
pub(crate) fn is_mobi(head: &[u8]) -> bool {
    head.get(60..68) == Some(b"BOOKMOBI")
}

impl Plugin for MobiFormat {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "MOBI books", paperpaw_sdk::package_version!())
            .description("Opens .mobi and .azw files without DRM.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each book, moved and removed with it.
        r.add_book_cache(Env::plugin_dir(PLUGIN.as_str()), "")?;
        r.add_format(
            FormatSpec::new(
                FORMAT,
                "format-mobi.title",
                vec![
                    Cow::Borrowed("mobi"),
                    Cow::Borrowed("azw"),
                    Cow::Borrowed("azw3"),
                ],
                Box::new(document::opener),
            )
            .sniff(is_mobi),
        )?;
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
