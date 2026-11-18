//! Buttons and gestures: what every swipe in from an edge, swipe, tap
//! zone, held tap, button and held button does, changed on the device
//! (Settings › Device).
//!
//! The first row chooses where a change applies: everywhere, on the home
//! screen, while reading, in the library, or in one tool. For a place
//! other than everywhere the second row says how it takes gestures: as
//! standard, its own (gestures go to the tool, a swipe in from an edge
//! needs a second one: for games and drawing), or none. A tool sets its
//! own default with `gestures.policy` in its key map; the user's choice
//! wins.
//!
//! The inputs follow in sections. Tapping one lists the actions to choose
//! from, in groups and searchable, after *As standard* (what plugins and
//! the button layout bind) and *Nothing*. Changes go to `paperpaw.ini`
//! like any binding written there and apply at once.
//!
//! Where there are buttons only (the X4), a change that leaves no button
//! to choose, to go back or to move one way or the other is refused: one
//! wrong choice would leave the device unusable, with no way back. The
//! last row, *Reset buttons and gestures*, takes every change back.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeKeys`]; the
//! interface-6 plugin is gone.

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

/// What the screens decide, without the host.
pub mod model;
mod native_data;
mod native_entry;
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
