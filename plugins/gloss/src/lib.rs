//! Gloss: small text above the words of a book, for those who learn a
//! language: how a word is read (furigana, pinyin, a pronunciation) or what it
//! means in a word or two.
//!
//! The words come from a list in the plugin's data folder
//! (`.paperpaw/plugins/gloss/`): `<book's file name without its extension>.txt`
//! for one book, else `<language>.txt` (`de.txt`) for every book in that
//! language. A line each: the word, a tab, the small text; `#` starts a
//! comment. The list is read once when a book opens and kept in memory;
//! the reader asks for each paragraph once while the book is open (the text
//! stage, `docs/abi/page-layers.md`), so a page turn reads nothing. The
//! setting *Size of the small text* is under Reading aids.
//!
//! This is the interface-7 plugin: no screens, no events beyond the stage.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the tests also use the builders"
)]
pub mod declarations;
pub mod glossary;
mod native_entry;
/// The interface-7 plugin.
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod tests;
