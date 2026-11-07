//! First-time help: the hints plugins name for their screens (see
//! [`paperpaw_sdk::help`]), shown over a screen the first time it opens
//! on this device, once: its zones framed with what a tap there does, the
//! rest in a box. A tap or a button closes them; *No more hints* switches
//! them all off (`help.show`, on at first). *Show hints again*, in the
//! system settings, has each shown once more.
//!
//! The screens shown are kept in `seen.txt`, a screen's id a line
//! (version 1).

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
mod layout;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;
mod seen;

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
