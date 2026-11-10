//! Reading profiles: the settings for reading, the display and the status
//! lines saved under a name ("Night", "Commute", "Large print") and
//! applied again with a tap.
//!
//! - *Profiles* (in the reader's menu under *More*, and in Settings ›
//!   Customise) lists them; the one in use is marked. Tapping one applies
//!   it; holding it offers to save the current settings into it, rename
//!   it or remove it. *Save as new profile* asks for a name.
//! - *Next profile* applies the profile after the one applied last: bind
//!   it to a button or use it in a rule.
//! - Names are unique (whatever their case): a name in use is refused,
//!   so that every profile can be told apart and *Next profile* goes
//!   round them all.
//! - Where the profiles cannot be read, the list says so and saves
//!   nothing over them.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeProfiles`]; the
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

mod native_data;
mod native_entry;
/// The file of profiles and their values, without the host.
pub mod store;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

/// Catalog sources published before native screen callbacks.
pub static CATALOGS: [(&str, &str); 2] = [
    ("en", include_str!("../i18n/en.ini")),
    ("de", include_str!("../i18n/de.ini")),
];

/// How it works, by language.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
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
