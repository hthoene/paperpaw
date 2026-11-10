//! The first-run setup: on the first start, a short guide through the
//! settings that matter, one a page, each with a sensible default already
//! chosen, so *Next* is always right.
//!
//! - The steps come from the plugins (the registry's setup steps): the
//!   language, the buttons, the text size, the time zone, Wi-Fi, the
//!   keyboard, the screen lock, and any other plugin's. Those this device
//!   does not need are left out (e.g. Wi-Fi where the system runs it).
//! - Skipped as a whole on its first page (by *Back* after a question),
//!   never shown again once done or skipped; *Set up PaperPaw* in the
//!   settings opens it again.
//! - The last page tells where books come from.
//! - It is the screen of the slot `core.setup`, so another plugin can
//!   replace it.
//!
//! Not shown on a device that was used before it came (a book was
//! opened already), nor where the storage cannot be written: it would
//! come again at every start, its choices lost.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeSetup`]; the
//! interface-6 plugin is gone.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
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
mod native_guide;
mod native_layout;
mod native_state;
mod native_steps;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

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
