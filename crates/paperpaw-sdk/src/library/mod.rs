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
pub mod what {
}

use alloc::format;
use alloc::string::{String, ToString};

use crate::Storage;
use paperpaw_digest::fnv1a64;








/// The key that names what is kept for the book at `path`: 16
/// hexadecimal digits of its hash. Folders of [`BookData`] name their
/// entries by it.
pub fn key(path: &str) -> String {
    let mut digits = [0_u8; KEY_LEN];
    String::from(key_into(path, &mut digits))
}

/// A folder where a plugin keeps something for each book, named by the
/// book's [`key`] after `prefix`, alone or followed by `.` and more (a
/// file or a folder): `cache/0123456789abcdef`, `epub-0123…/`,
/// `0123….ini`; in the folder itself, or in the part of the folder of
/// the key's first two digits (`01/0123…`), where a folder would hold
/// thousands (a memory card looks through a folder one entry after the
/// other for a name). Registered with
/// `Items::add_book_data` or
/// `Items::add_book_cache`, the
/// kernel moves it with its book (`move_data`), removes it with it
/// (`remove_data`) and when its book is no longer there
/// ([`clean_up_unused`]): a cache by itself, the reader's data only when
/// the reader says so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BookData {
    pub folder: alloc::borrow::Cow<'static, str>,
    pub prefix: alloc::borrow::Cow<'static, str>,
    pub kind: BookDataKind,
}

impl BookData {
    /// Data of `kind` in `folder`, named `prefix` and the book's id.
    pub fn new(
        folder: impl Into<alloc::borrow::Cow<'static, str>>,
        prefix: impl Into<alloc::borrow::Cow<'static, str>>,
        kind: BookDataKind,
    ) -> Self {
        Self {
            folder: folder.into(),
            prefix: prefix.into(),
            kind,
        }
    }
}

/// What a folder of [`BookData`] holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookDataKind {
    /// The reader's: a place, notes, settings for the book, a sync's
    /// state. A book not found may be on a card not inserted, or renamed
    /// on a computer: its data goes only when the reader agrees.
    #[default]
    UserData,
    /// Made again from the book when needed (pages laid out, a cover made
    /// small): it goes by itself once its book is not found.
    Cache,
}

impl BookData {
    /// The book key entry `name` of the folder is named by, if it is one
    /// of these, and what follows it.
    pub fn key_of<'n>(&self, name: &'n str) -> Option<(&'n str, &'n str)> {
        let rest = name.strip_prefix(&*self.prefix)?;
        let key = rest.get(..16)?;
        let tail = &rest[16..];
        let hex = key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        (hex && (tail.is_empty() || tail.starts_with('.'))).then_some((key, tail))
    }

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

#[cfg(test)]
mod fallible_tests;

#[cfg(test)]
#[allow(
    clippy::let_underscore_must_use,
    reason = "tests set up files in memory, which does not fail"
)]
mod tests;
