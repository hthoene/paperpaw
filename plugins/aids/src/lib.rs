//! Reading aids, set under Settings › Reading › Reading aids and applied by
//! the reader through the `reader.filter` stage (`READER-FILTER.md`):
//!
//! - *Bold word beginnings*: the first half of every word in bold, which
//!   some readers follow more easily ("bionic reading").
//! - *Replace words*: pairs `from = to` separated by `;`, e.g. for old
//!   spellings or names; whole words only, as written.
//!
//! - *Pictures*: shown as the book has them, a short line in their place
//!   (the picture's description), or left out.
//!
//! All are off at first and apply to every book.

#![no_std]

extern crate alloc;

mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod pictures;

pub mod reader_filter;

/// Whether the character before `at` and the one from `end` are no part
/// of a word.
pub(crate) fn whole_word(text: &str, at: usize, end: usize) -> bool {
    let before = text[..at].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

#[cfg(test)]
mod filters_tests;
/// The interface-6 text filters, kept as the reference the stage's rules are
/// tested against (no plugin registers them any more).
#[cfg(test)]
mod oracle;
