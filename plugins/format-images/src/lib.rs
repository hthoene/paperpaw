//! Pictures as books for PaperPaw: a JPEG, PNG or GIF opens as a page, and
//! the pictures after it in its folder, in the order of their names
//! (numbers counted as numbers), as the pages after it.
//!
//! - **What is read:** `.jpg`, `.jpeg`, `.png` and `.gif` (hidden files are
//!   left out); each picture is a page, fit to the screen, and a comic's
//!   direction is not asked. The title is the file's name. The picture on
//!   the first page is the book's cover.
//! - **How:** the folder is listed once when a picture is opened (at most
//!   `MAX_PAGES` pages, the first in order), and a picture is decoded a
//!   few rows at a time when its page shows, into the plugin's folder
//!   for the book, so one picture is in memory at most. A file that is no
//!   picture, or that is too large, is a page with its name on it.
//! - **A folder as one book:** open its first picture. A picture later
//!   in the folder starts there, and its position is its own.
//!
//! Nothing here is in the interface: it is a format plugin like the
//! others, on the SDK only.

#![no_std]

extern crate alloc;

mod document;
mod names;
mod pictures;
pub mod provider;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec;

use paperpaw_sdk::{Env, FormatSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub use document::ImagesDocument;
pub use pictures::CACHE;

pub const PLUGIN: Id = Id::new_static("format-images");
pub const FORMAT: Id = Id::new_static("format-images.pictures");

/// Pages of a book at most: the first in the order of the names.
pub(crate) const MAX_PAGES: usize = 300;
/// The longest file name taken.
pub(crate) const MAX_NAME: usize = 255;
/// The largest picture file decoded.
pub(crate) const MAX_FILE: u64 = 24 << 20;

/// The pictures format plugin.
#[derive(Debug, Default)]
pub struct ImagesFormat;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for ImagesFormat {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Pictures", paperpaw_sdk::package_version!())
            .description("Opens pictures (.jpg, .png, .gif) as books, a page each.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is decoded of each book's pictures, moved and removed with it.
        r.add_book_cache(Env::plugin_dir(PLUGIN.as_str()), "")?;
        r.add_format(FormatSpec::new(
            FORMAT,
            "format-images.title",
            vec![
                Cow::Borrowed("jpg"),
                Cow::Borrowed("jpeg"),
                Cow::Borrowed("png"),
                Cow::Borrowed("gif"),
            ],
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
