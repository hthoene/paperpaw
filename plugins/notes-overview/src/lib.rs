//! All notes: the bookmarks and highlights the reader keeps,
//! seen across the library, and written out in Kindle's `My Clippings.txt`
//! form.
//!
//! *Tools › All notes* lists the books that have notes with how
//! many; a book opens its notes, a note opens the book there. At the end
//! of the list, *Export as My Clippings.txt* writes the notes of every book
//! to `Exports/My Clippings.txt` beside the library folder.
//!
//! It reads the library's books and their annotations through the library
//! service (`core.library`), a page at a time, so a large library costs
//! time, not memory (see `native_scan`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
pub mod export_file;
pub mod model;
mod native_entry;
pub mod native_export;
#[cfg(test)]
mod native_export_tests;
pub mod native_library;
pub mod native_notes;
#[cfg(test)]
mod native_notes_tests;
pub mod native_plugin;

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::SettingSpec;
    include!(concat!(env!("OUT_DIR"), "/notes_overview_settings.rs"));
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
pub mod native_run;
pub mod native_scan;
#[cfg(test)]
mod native_scan_tests;
pub mod native_staged;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

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
