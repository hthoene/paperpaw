//! The settings screens, generated from the settings every plugin declares
//! (`Registrar::add_setting`), so no plugin needs its own settings UI.
//!
//! The plugin is native (interface 7): [`native_plugin`] holds the screens,
//! [`native_secrets`] the passwords and keys, [`native_model`] what the pages
//! are made of without the host, and `NATIVE_ENTRY` (feature `linked`) the
//! entry the app links. See `PORTING-7.md`.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
mod native_data;
mod native_entry;
pub mod native_model;
pub mod native_secrets;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;
/// The interface-7 plugin.
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// Catalog sources published before native screen callbacks.
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
