//! Edge light: swipe up or down along the left or right edge of the screen to
//! set the front light, brightness on one side and warmth on the other.
//!
//! - An input filter (`core.input-filter`) sees every finger first. It lets all
//!   of them pass; one that goes down in a strip along an edge and moves up or
//!   down is followed, and the light is set from how far it has moved
//!   (`swipe`). Page swipes and taps keep working everywhere, also in the strip;
//!   the vertical swipe a finger of ours ends with is the only gesture taken.
//! - Each step (5 %) is a setting change, with a small notice of the new level; the
//!   filter answers *run* and the action `edge-light.step` does it, as a filter sends
//!   no command itself.
//! - The widget *Front light* shows the level and a tap switches the light on or
//!   off; so does the action `edge-light.toggle`.
//!
//! Only on devices with a touch screen and a front light. The plugin is a native
//! interface-7 plugin (`native_plugin`); what it decides without a host is in
//! `swipe` (`PORTING-7.md`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
pub mod declarations;
mod native_entry;
pub mod native_plugin;
mod native_widget;
pub mod swipe;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("edge-light");

/// Help text published by the host during atomic native admission.
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];
/// Catalog sources published before native callbacks.
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
