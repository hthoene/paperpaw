//! The menu overlay.
//!
//! Shows the menu named by the `menu` parameter at the bottom of the screen,
//! above whatever is open. Entries come from every plugin (see
//! `Registrar::add_menu_entry`) or from the user's `[menus]` definitions.
//! Choosing an action closes the menu and runs it. Settings show their
//! current value and change in place, so the menu stays open: switches
//! toggle, numbers have − and + buttons, choices open a list. An entry
//! whose action is named like a menu with entries opens that menu as a
//! submenu above this one.
//!
//! Bound to the `core.menu` slot; replace it to get a different menu style
//! (radial, side bar, …) for every menu at once. The same plugin shows
//! menus as panels of controls (`core.panel`, see `panel`).

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
pub mod native_bar;
mod native_bar_screen;
mod native_data;
mod native_entry;
pub mod native_model;
pub mod native_panel;
pub mod native_panel_screen;
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/menu_settings.rs"));
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
