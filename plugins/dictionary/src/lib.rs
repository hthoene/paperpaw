//! The dictionary: looks up a word in the StarDict dictionaries of a
//! folder and shows the definitions in a panel over the lower part of the
//! screen.
//!
//! Bound to the `core.lookup` slot, which the reader opens for a word
//! (long press, or *Look up a word* in its menu). Dictionaries are folders
//! with `.ifo`, `.idx`, `.dict(.dz)` and optional `.syn` files, as used by
//! KOReader, GoldenDict and many e-readers; they are found anywhere below
//! the dictionary folder. The first lookup prepares a small index for each
//! dictionary in the background; lookups then take a few milliseconds and
//! little memory (see `paperpaw-stardict`).
//!
//! The panel also lists what other plugins' lookup providers find (see
//! `paperpaw_sdk::lookup`): those that work offline with every word,
//! those that go online (Wikipedia's) when *Online* is chosen. An entry
//! that has more to show ends with a line that opens it (*Open
//! Wikipedia ›*). The StarDict dictionaries are a provider too, for a
//! lookup screen of another plugin.
//!
//! Buttons: next/previous page or up/down scroll long definitions, confirm
//! opens the first entry's more on the page, else closes, as back does.
//! Touch: tap a line that opens more, else the right part to scroll, the
//! left part to go back, outside to close.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
#[doc(hidden)]
pub mod native_asking;
#[doc(hidden)]
pub mod native_disk;
mod native_entry;
#[doc(hidden)]
pub mod native_folders;
#[doc(hidden)]
pub mod native_model;
#[doc(hidden)]
pub mod native_online;
#[doc(hidden)]
pub mod native_panel;
/// The interface-7 dictionary (slice 1; see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_popup;
#[doc(hidden)]
pub mod native_source;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/dictionary_settings.rs"));
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

use paperpaw_sdk::Id;
use paperpaw_sdk::setting::{Setting, Text};

pub const PLUGIN: Id = Id::new_static("dictionary");
/// The panel with the definitions; parameters `word` and `language`.
pub const POPUP: Id = Id::new_static("dictionary.popup");
/// The folder searched for dictionaries.
pub const FOLDER: Setting<Text> = Setting::new("dictionary.folder");
/// The StarDict dictionaries as a lookup provider.
pub const PROVIDER: Id = Id::new_static("dictionary.stardict");

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
