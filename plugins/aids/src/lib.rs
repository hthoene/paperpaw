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

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/aids_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;

/// The setting groups the plugin declares, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_GROUPS: &[paperpaw_sdk::SettingGroupSpec] = &native_settings::GROUPS;

/// The full ids the plugin declares, sorted, as a table kept in the program
/// like [`NATIVE_SETTINGS`] (the kernel takes them in place of copies).
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;
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
