//! A diagnostics screen for bringing up new devices and for testing the
//! kernel: it shows every gray level of the panel, exercises button and
//! touch focus navigation, and demonstrates partial versus full refreshes.
//!
//! Page buttons turn its test pages (as many as the setting *Number of
//! test pages* says); a held *Select* or a held finger starts it over.
//!
//! A guided device check (`check`) walks through every part of a device one
//! page at a time, for bringing up new devices: the screen, buttons, touch,
//! light, battery, card, clock, Wi-Fi, sleep and memory. It writes a report to
//! the plugin's data folder to paste into a pull request ([`steps::REPORT`]).
//! It is under Tools where PaperPaw is the device's firmware, which has this
//! plugin in its builds for developers alone; elsewhere the action
//! `diagnostics.open-check` opens it.
//!
//! The test screen binds itself to no slot: a user binds it explicitly:
//!
//! ```ini
//! [slots]
//! core.home = diagnostics.screen
//! ```
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeDiagnostics`].

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

mod check;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod steps;
pub mod test_screen;

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
