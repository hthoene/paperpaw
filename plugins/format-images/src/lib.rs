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
//!   few rows at a time when its page shows, into the document cache
//!   of the book, so one picture is in memory at most. A file that is no
//!   picture, or that is too large, is a page with its name on it.
//! - **A folder as one book:** open its first picture. A picture later
//!   in the folder starts there, and its position is its own.
//!
//! It is an interface-7 plugin only: `NativeImages` declares the format and
//! `ImagesProvider` answers the host's `core.format` calls; the decoded
//! pictures are kept in the host's document cache.

#![no_std]

extern crate alloc;

mod document;
mod names;
mod pictures;
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

use paperpaw_sdk::Id;

pub use document::ImagesDocument;
pub use pictures::CACHE;

pub const PLUGIN: Id = Id::new_static("format-images");

/// Pages of a book at most: the first in the order of the names.
pub(crate) const MAX_PAGES: usize = 300;
/// The longest file name taken.
pub(crate) const MAX_NAME: usize = 255;
/// The largest picture file decoded.
pub(crate) const MAX_FILE: u64 = 24 << 20;

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
