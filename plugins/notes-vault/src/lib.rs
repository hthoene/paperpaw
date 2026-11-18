//! Notes vault: a folder of Markdown notes read on the device, as a vault
//! of notes keeps them (it works with Obsidian vaults: plain `.md` files in
//! folders, with `[[wikilinks]]`, front matter and `#tags`).
//!
//! - *Notes vault* in the tools: the vault's folders, its latest changes,
//!   its tags, and a search in the notes' titles. A note opens in the
//!   reader; its links to other notes (by name, as a vault writes them)
//!   open them, and *Back* returns.
//! - Home widgets: the recent notes, a pinned note, a note of the day, a
//!   random note, the tags, and a search (see `native_widgets`).
//!
//! The vault is read as it is on the card, never changed. Its folders are
//! listed as they are; the latest changes, the tags and the search come
//! from an index of the notes made in the background, a piece at a time
//! while the vault's page is open, and kept in a file (see `native_model::index`): a
//! vault of thousands of notes costs time the first time, and a window of
//! rows of memory, never its size.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod fallible;
mod markdown;
mod native_card;
mod native_entry;
#[doc(hidden)]
pub mod native_format;
mod native_home;
mod native_list;
#[doc(hidden)]
pub mod native_model;
mod native_notice;
mod native_paged;
/// The interface-7 notes vault (see `PORTING-7.md`).
pub mod native_plugin;
mod native_tags;
mod native_widgets;

pub use markdown::MarkdownDocument;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_golden_tests;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("notes-vault");

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

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/notes_vault_settings.rs"));
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

/// The ids the plugin's declarations name, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;
