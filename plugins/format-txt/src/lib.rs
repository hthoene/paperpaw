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

mod document;
pub mod preparation;
mod utf16;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec;

use paperpaw_sdk::document::Document;
use paperpaw_sdk::{Env, FormatSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub use document::TxtDocument;

pub const PLUGIN: Id = Id::new_static("format-txt");
pub const FORMAT: Id = Id::new_static("format-txt.txt");

/// The plain text format plugin.
#[derive(Debug, Default)]
pub struct TxtFormat;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for TxtFormat {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Plain text", paperpaw_sdk::package_version!())
            .description("Opens plain text files (.txt).")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Copies in UTF-8 of UTF-16 files, moved and removed with them.
        r.add_book_cache(Env::plugin_dir(PLUGIN.as_str()), "")?;
        r.add_format(FormatSpec::new(
            FORMAT,
            "format-txt.title",
            vec![Cow::Borrowed("txt"), Cow::Borrowed("text")],
            Box::new(|storage, path, _| {
                TxtDocument::open(storage, path).map(|d| Box::new(d) as Box<dyn Document>)
            }),
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
