//! The picture home screen: your own picture behind the widgets, pages of
//! widgets and tools placed anywhere in a grid, and a status bar that is
//! yours to configure. A store plugin: while it is on, it is the home
//! screen (`core.home`); switched off, the built-in home screen is back,
//! with its own pages.
//!
//! - **Picture**: chosen from a folder on the card; fill, whole, stretch,
//!   original size or tiled, in one of nine places, zoomed, paler; made
//!   once into a screen-sized gray file (`picture`) and drawn from it.
//! - **Widgets**: any plugin's, on plates over the picture, in the grid
//!   the built-in home screen uses (`paperpaw_sdk::widgets::grid`): drag
//!   to move, by the corner to size, tap for the options.
//! - **Bars**: the status bar (top or bottom, full or compact, on a plate
//!   of five looks or on the picture) and the row of hints below (on the
//!   X4), each always, never, or for a few seconds after a press; the press
//!   that shows them does nothing else.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

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
    include!(concat!(env!("OUT_DIR"), "/picture_home_settings.rs"));
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
pub mod native_backdrop;
pub mod native_backdrop_host;
pub mod native_bar;
pub mod native_dash;
pub mod native_dashboard;
pub use paperpaw_ui_kit::dashboard::focus as native_focus;
pub use paperpaw_ui_kit::dashboard::grid as native_grid;
pub mod native_icons;
pub mod native_measure;
pub mod native_pages;
pub mod native_pick;
pub mod native_picture;
pub mod native_plugin;
pub mod native_shown;
pub mod native_source;
pub mod native_stage;
mod settings;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("picture-home");
/// The home screen.
pub const SCREEN: Id = Id::new_static("picture-home.dashboard");
/// Its settings' group (Settings, Customise, Picture home screen).
pub const GROUP: Id = Id::new_static("picture-home.group");

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
