//! The first-run setup: on the first start, a short guide through the
//! settings that matter, one a page, each with a sensible default already
//! chosen, so *Next* is always right.
//!
//! - The steps come from the plugins (`setup.step` declarations, listed by
//!   the registry): the language, the buttons, the text size, the time zone,
//!   Wi-Fi, the keyboard, the screen lock, and any other plugin's. Those this
//!   device does not need are left out by the host (e.g. Wi-Fi where the
//!   system runs it).
//! - Skipped as a whole on its first page (by *Back* after a question), never
//!   shown again once done or skipped; *Set up PaperPaw* in the settings opens
//!   it again.
//! - The last page tells where books come from.
//! - It is the screen of the slot `core.setup`, so another plugin can replace it.
//!
//! Not shown on a device that was used before it came (a book was opened
//! already), nor where the storage cannot be written: it would come again at
//! every start, its choices lost (the kernel tells why, e.g. a card that is
//! not FAT32).
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeSetup`].

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

mod data;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
pub mod model;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod view;

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
