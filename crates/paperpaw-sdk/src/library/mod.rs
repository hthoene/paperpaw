//! The library as a service: what is known about books beyond their
//! files, for every plugin that shows or uses them (the library, the home
//! screen, the sleep screen, syncs). Data and the screens that show it
//! are apart: any plugin may offer another view of the same books.
//!
//! - A book's details: title, author, series, tags and cover, read from
//!   the book once (`index`), its reading progress, and fields that
//!   plugins add (e.g. a rating or where a book came from), in
//!   `.paperpaw/library/details/<h>/<hash>.ini`, `<h>` the first two
//!   digits of the hash; its cover made small beside, in
//!   `.paperpaw/library/thumbs/<h>/<hash>.thumb`. (Earlier versions kept
//!   them in `books/<hash>.ini` and `covers/<hash>.thumb`: read there
//!   until the kernel moved them, `split`.)
//! - The books opened last, most recent first (`opened.txt`), and the
//!   favourites (`favourites.txt`).
//! - The catalog of all books under the library folder ([`catalog`]),
//!   and listings of a folder's books (`listing`).
//!
//! The kernel keeps them up to date from the standard reading events and
//! moves them with a book ([`record`], [`moved`]). Nothing here grows
//! with the size of the library: the lists are capped, and details are
//! read a book at a time.

#![deny(clippy::let_underscore_must_use)]

mod carry;
pub mod catalog;
mod details;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod emptiness;
mod fallible;
mod fingerprint;
pub mod index;
mod indexed;
pub(crate) mod listing;
pub(crate) mod metadata;
pub mod sorted;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod split;

pub use fallible::{KEY_LEN, key_into};
pub use fingerprint::Fingerprint;

/// What changed of a book ([`events::BookChanged::what`](crate::builtin::events::BookChanged)):
/// bits, any together.
pub mod what {}

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_digest::fnv1a64;

/// The key that names what is kept for the book at `path`: 16
/// hexadecimal digits of its hash. Folders of [`BookData`] name their
/// entries by it.
pub fn key(path: &str) -> String {
    let mut digits = [0_u8; KEY_LEN];
    String::from(key_into(path, &mut digits))
}

/// The part of folder `folder` that what is kept by `key` goes into:
/// the subfolder of its first two digits (of 256), so that no folder holds
/// thousands of entries, which a memory card looks through one after the
/// other for a name.
pub(crate) fn split(folder: &str, key: &str) -> String {
    format!("{folder}/{}", key.get(..2).unwrap_or("00"))
}

/// Where a plugin keeps what it keeps for the book at `book` in folder
/// `folder` of [`BookData`] (registered with
/// `Items::add_book_cache` or
/// `Items::add_book_data`), named `prefix` and
/// the book's [`key`]: in the part of the folder of the key's first two
/// digits, `<folder>/<2 digits>/<prefix><key>`, so that no folder holds
/// thousands of entries (a memory card looks through a folder one entry
/// after the other for a name). The kernel moves, removes and cleans up
/// what is there, and what earlier plugins kept in the folder itself.
#[must_use]
pub fn kept_path(folder: &str, prefix: &str, book: &str) -> String {
    let key = key(book);
    format!(
        "{}/{prefix}{key}",
        split(folder.trim_end_matches('/'), &key)
    )
}

/// The command that tells everyone something of the book at `path`
/// changed: `what`, bits of [`what`]
/// ([`events::BOOK_CHANGED`](crate::builtin::events::BOOK_CHANGED)).
pub fn changed_book(path: &str, what: u32) -> crate::Command {
    crate::builtin::events::BOOK_CHANGED.emit(crate::builtin::events::BookChanged {
        path: path.to_string(),
        what,
    })
}
