//! The sleep screen (slot `core.sleep`): what the screen shows while the
//! device sleeps, e-ink keeping it without power. An interface-7 native
//! plugin ([`native_plugin`]).
//!
//! Setting `sleep.show`:
//!
//! - *Sleeping cat* (the default): PaperPaw's drawing of a cat asleep,
//!   in the middle of the screen on white (see [`cat`]);
//! - *Cover*: the cover of the book read last;
//! - *Picture*: the pictures (JPEG, PNG) of a folder (`sleep.folder`,
//!   `sleep` at first, as on CrossPoint), one after the other;
//! - *Message*: PaperPaw's logo and "Sleeping";
//! - *Widgets*: widgets as on the home screen (setting `sleep.widgets`, a
//!   place of widgets above each other, edited on the slot
//!   `core.edit-widgets`), e.g. the book being read and today's reading,
//!   in the middle of the screen;
//! - *Page*: the screen stays as it is;
//! - *Page with a picture*: the page stays, a picture or a widget over
//!   it (picked on the screen `pick-overlay`, see [`native_picker`]); where
//!   and how large it is, how much the page shows through it and when it is
//!   used are settings ([`native_sleep`]);
//! - a mode of another plugin: it offers a screen of its own for the slot
//!   `core.sleep` and a `setting.choice` of `sleep.show` naming it, which the
//!   host shows in the place of this plugin's screen, and which this plugin
//!   binds the slot to (`native_plugin`).
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
mod native_folder;
mod native_pick;
mod native_picker;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_screen;
pub mod native_sleep;
mod native_stack;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// PaperPaw's sleeping cat: made for PaperPaw (see `NOTICE`), shipped in the
/// plugin, written once into the plugin's data folder and decoded by the host
/// for the screen's size (`native_screen`).
pub mod cat {
    /// The drawing: 760 × 501 pixels, 16 grays, cropped to the cat.
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
