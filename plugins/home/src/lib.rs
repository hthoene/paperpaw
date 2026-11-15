//! The home screen: pages of widgets and tools in a grid, like a phone's.
//!
//! - **Pages**: each a grid of cells (4 columns by default, rows to fit
//!   the screen) in a setting of its own (the grid is the
//!   SDK's, `paperpaw_sdk::widgets::grid`): by default
//!   the clock, the book being read and the recent ones, then a page with
//!   all tools. The status line sits above the grid. Swipes and the page
//!   buttons go from page to page; dots below show where one is.
//! - **Items**: widgets (the home screen's own and any
//!   plugin's, `Registrar::add_widget`) and tools (actions, as icons),
//!   each taking a rectangle of cells; empty cells stay empty.
//! - **Targets**: what an item does when tapped (open a book, a tool).
//!   With buttons, left and right (or up and down) go from target to
//!   target and confirm runs it; the first press shows where the focus is.
//! - **Editing**: the pencil below the page (reached with the buttons too),
//!   a long press or *Edit home screen* in the main menu (`home.edit-pages`,
//!   also where the pencil is switched off) shows the grid: drag an item to move it, by its
//!   corner to size it; tap it for its options (move or size it with the
//!   buttons, its settings, remove it). *Add* lists what can be added,
//!   and adds or removes pages.
//! - **Tools**: the entries of the `home.actions` menu (the library by
//!   default) and of the tools menu; plugins add their own with
//!   `Registrar::add_menu_entry`.
//!
//! The recently read books and their covers come from the library service
//! (`paperpaw_sdk::library`), which follows the standard reading events,
//! so any reader plugin works.
//! The home screen is bound to `core.home`; replace it with any other
//! screen by configuration or with a plugin of higher priority.

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
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/home_settings.rs"));
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
pub mod native_add;
pub mod native_add_screen;
pub use paperpaw_ui_kit::dashboard::arrange as native_arrange;
pub mod native_card;
pub mod native_clock;
pub mod native_dash;
pub mod native_dashboard;
pub mod native_date;
pub mod native_drawer;
pub use paperpaw_ui_kit::dashboard::focus as native_focus;
pub mod native_gallery;
pub use paperpaw_ui_kit::dashboard::grid as native_grid;
pub mod native_icons;
pub mod native_item_settings;
pub mod native_lines;
pub mod native_made;
pub mod native_measure;
pub mod native_pages;
pub mod native_plugin;
pub mod native_recent;
pub mod native_reflow;
pub mod native_shelf;
pub mod native_shown;
pub mod native_source;
pub mod native_status;
pub mod native_widgets;

/// The home screen's id (the one `core.home` is bound to).
pub const SCREEN: paperpaw_sdk::Id = paperpaw_sdk::Id::new_static("home.dashboard");

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
