//! Power: which power mode is worn (ADR 0025). The reader chooses
//! Normal, Saving, Offline, or Automatic, which goes by the battery:
//! Saving below one level, Offline below another, Normal while charging.
//! The modes themselves are presets of the kind `core.power-mode`, worn by
//! the kernel (`command.wear`); this plugin only decides which, silently.
//!
//! - *Power* in the quick panel: the four as segments, by buttons and by
//!   touch alike.
//! - Settings › System › Power: the mode, Automatic's two levels and
//!   *Normal while charging*.
//! - A status item, *Power mode*: nothing in Normal, so the lines stay calm.
//! - The power button held opens a small menu: sleep, switch off where
//!   PaperPaw does so, restart, lock or unlock touch.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativePower`]; what
//! it decides without a host is in `model`.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

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
