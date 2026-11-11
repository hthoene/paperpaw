//! News: feeds (RSS or Atom) fetched as books to read, one a feed, in the
//! `News` folder of the library. The plugin is a native interface-7 plugin
//! only (`native_plugin`); what it decides without a host is in `native_news`,
//! `native_convert`, `native_book` and `native_fetch` (`PORTING-7.md`).
//!
//! *News* in the main menu lists the feeds: *Fetch the news* fetches all
//! of them, a feed is opened with a tap, and holding it offers to fetch
//! it alone or to remove it; *Add a feed* asks for its address (which
//! can come from a phone too, see the keyboard). Each feed becomes a page
//! with its newest articles as plain text (see `native_convert`), replaced by
//! the next fetch. Feeds are fetched one after the other and each is
//! read as it streams from storage, so this works on small devices too.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
pub mod declarations;
pub mod native_book;
pub mod native_books;
pub mod native_convert;
mod native_entry;
pub mod native_fetch;
pub mod native_news;
pub mod native_plugin;
mod native_screen;
mod native_shelf;
mod native_widgets;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/news_settings.rs"));
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

pub const PLUGIN: paperpaw_sdk::Id = paperpaw_sdk::Id::new_static("news");

/// Help text published by the host during atomic native admission.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

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

#[cfg(test)]
mod native_book_tests;
#[cfg(test)]
mod native_convert_hostile_tests;
#[cfg(test)]
mod native_convert_tests;
#[cfg(test)]
mod native_fetch_tests;
#[cfg(test)]
mod native_golden;
#[cfg(test)]
mod native_rules_tests;
#[cfg(test)]
mod test_volume;
