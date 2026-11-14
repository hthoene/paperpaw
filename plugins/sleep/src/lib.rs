//! The sleep screen (slot `core.sleep`): what the screen shows while the
//! device sleeps, e-ink keeping it without power.
//!
//! Setting `sleep.show`:
//!
//! - *Sleeping cat* (the default): PaperPaw's drawing of a cat asleep,
//!   in the middle of the screen on white (see `cat`);
//! - *Cover*: the cover of the book read last;
//! - *Picture*: the pictures (JPEG, PNG) of a folder (`sleep.folder`,
//!   `sleep` at first, as on CrossPoint), one after the other;
//! - *Message*: PaperPaw's logo and "Sleeping" (the logo as set,
//!   see [`paperpaw_sdk::logo`]);
//! - *Widgets*: widgets as on the home screen (setting `sleep.widgets`, a
//!   place of widgets above each other, edited on the slot
//!   `core.edit-widgets`), e.g. the book being read and today's reading,
//!   in the middle of the screen;
//! - *Page*: the screen stays as it is;
//! - *Page with a picture*: the page stays, a picture or a widget over
//!   it, its clear parts showing the page;
//! - a mode of another plugin: it offers a screen of its own among the
//!   choices (`Registrar::add_choice(sleep::SHOW, "my.sleep-screen")`, the
//!   value the screen's id and the translation key of its name), which
//!   the sleep screen shows in its place, sleeping as it does.
//!
//! Without a cover or a picture the cat shows. Where the device is
//! off while it sleeps (the X4), this is also what shows as it starts.
//! Asleep, the screen takes every button but *Sleep* and *Home*. It
//! closes when the device wakes, wherever it is: what opened above it as
//! the device woke (a lock, a review of flash cards) stays. Only where PaperPaw
//! shows the sleep screen itself (the X4, not Android): elsewhere its
//! settings are hidden.

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
/// The picker of what goes over the page, without the host.
pub mod native_pick;
mod native_picker;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_screen;
pub mod native_sleep;
mod native_stack;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/sleep_settings.rs"));
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

/// The sleeping cat: the drawing built into the program, so that the screen
/// has something to show before any file can be read.
pub mod cat {
    /// The drawing: 760 × 501 pixels, 16 grays stored in 4 bits each
    /// (#258), cropped to the cat.
    pub const PICTURE: &[u8] = include_bytes!("../pictures/sleeping-cat.png");
}

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
