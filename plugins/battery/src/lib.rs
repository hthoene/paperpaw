//! Battery use: records the battery while the device is awake, with what
//! it did meanwhile (screen refreshes, input, pages turned), and shows how
//! much it uses an hour awake and for 100 pages, and how long it lasts.
//!
//! The record is a log on the device's storage (see its `log` module), kept for
//! analysis on a computer too: where the battery goes is what to make
//! leaner next.

#![no_std]

extern crate alloc;

mod log;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
pub mod native_plugin;
mod native_view;
pub mod recording;

use paperpaw_sdk::{Id, setting::Setting};

pub const PLUGIN: Id = Id::new_static("battery");
/// Records the battery's use.
pub const ENABLED: Setting<bool> = Setting::new("battery.enabled");
/// Shows what each plugin took (for developers).
pub const BY_PLUGIN: Setting<bool> = Setting::new("battery.by-plugin");
/// Opens the view.
pub const OPEN: Id = Id::new_static("battery.open");

/// Help text published by the host during atomic native admission.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];
/// Catalog sources published before native Started/screen callbacks.
pub static CATALOGS: [(&str, &str); 2] = [
    ("en", include_str!("../i18n/en.ini")),
    ("de", include_str!("../i18n/de.ini")),
];

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
