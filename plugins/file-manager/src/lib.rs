//! A file manager: every folder and file of the device's storage, to
//! open, rename, move, copy and delete, and to make folders.
//!
//! - Tapping a folder opens it, *Back* goes to the folder above.
//! - Tapping a file, or holding any entry (with buttons: a long press on
//!   confirm), offers what can be done with it; deleting asks first.
//! - *Move* and *Copy* pick the entry: then go to the folder it belongs in
//!   and choose *Paste here* in the bar at the bottom.
//! - Files starting with `.` (e.g. PaperPaw's own data) are hidden unless
//!   the setting *Show hidden files* says otherwise.
//!
//! - A file the library takes for a book also offers *Open* (in the reading app
//!   the user chose) and, where more than one reads it, *Open with*; deleting a
//!   book deletes what is kept for it (its place, notes and data) with it.
//!
//! With buttons only, down from the last entry reaches the bar.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;
#[cfg(test)]
#[global_allocator]
static COUNTING: model::testing::Counting = model::testing::Counting;

mod browser;
mod card;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
pub mod model;
mod native_draw;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/file_manager_settings.rs"));
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
pub static NATIVE_TITLES: &[(paperpaw_sdk::Id, &str)] = &native_settings::TITLES;
/// The interface-7 plugin.
pub mod native_plugin;

/// The plugin's id.
pub const PLUGIN: &str = "file-manager";

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
