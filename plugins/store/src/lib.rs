//! The plugin store: the plugins of a plugin library (paperpaw.org's by
//! default) for this device, by group as the website shows them, with a
//! search, each with a page to install, update or remove it (ADR 0023).
//! Fonts are one more entry, which opens the fonts browser where it is
//! installed.
//!
//! The list is read as it streams into an index kept on the storage, so
//! it may be as long as it likes; a plugin's files go straight to the
//! storage, one after another, each checked ([`paperpaw_sdk::install`]).
//! Plugins with code are listed where PaperPaw loads them and a build
//! fits; their code is checked whole and signed by its author (`trust`,
//! ADR 0024). Built-in plugins are not listed, nor a plugin that would
//! take the place of one: those come with the firmware.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
#[doc(hidden)]
pub mod native_ask;
mod native_asks;
#[doc(hidden)]
pub mod native_device;
#[doc(hidden)]
pub mod native_download;
mod native_entry;
mod native_folders;
#[doc(hidden)]
pub mod native_fonts;
mod native_fonts_host;
mod native_glue;
#[doc(hidden)]
pub mod native_index;
#[doc(hidden)]
pub mod native_install;
#[doc(hidden)]
pub mod native_installs;
#[doc(hidden)]
pub mod native_item;
mod native_jobs;
#[doc(hidden)]
pub mod native_list;
#[doc(hidden)]
pub mod native_load;
#[doc(hidden)]
pub mod native_model;
#[doc(hidden)]
pub mod native_page;
/// The interface-7 plugin store (slice 1; see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_queue;
#[doc(hidden)]
pub mod native_refusal;
#[doc(hidden)]
pub mod native_request;
#[doc(hidden)]
pub mod native_screen_fetch;
#[doc(hidden)]
pub mod native_screen_letters;
#[doc(hidden)]
pub mod native_screen_list;
#[doc(hidden)]
pub mod native_screen_offer;
#[doc(hidden)]
pub mod native_screen_page;
#[doc(hidden)]
pub mod native_screen_shots;
#[doc(hidden)]
pub mod native_screen_source;
#[doc(hidden)]
pub mod native_screen_view;
#[doc(hidden)]
pub mod native_screens;
#[doc(hidden)]
pub mod native_sort;
#[doc(hidden)]
pub mod native_wire;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/store_settings.rs"));
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
