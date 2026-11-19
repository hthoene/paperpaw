//! Simple home: a launcher for those who need only the core. A large clock,
//! the card of the book being read, a card each for the library and the store,
//! a dock of three icons; a quick panel of big round switches with bars for the
//! light and the text size; the library as a list of cards, no covers.
//!
//! - The home screen is offered for `core.home` and the panel for `core.panel`:
//!   the reader picks them in Settings like any home. The current screens stay
//!   as they are.
//! - Which cards and in which order, which icons the dock holds and which
//!   switches and bars the panel shows are three menus (`launcher-simple.cards`,
//!   `.dock`, `.panel`) edited in Settings, *Edit menus*: any plugin's action,
//!   as for the main menu. *Simple home* in Settings opens the three.
//! - A key press moves the focus and draws the two parts it touches; the minute's
//!   tick draws the clock alone; nothing is read from the card by a key.
//!
//! The plugin is a native interface-7 plugin (`native_plugin`); what it decides
//! without a host is in `model` (`PORTING-7.md`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod customise;
mod data;
#[path = "../declarations.rs"]
pub mod declarations;
mod dock;
mod home;
mod library;
mod look;
pub mod model;
mod native_entry;
pub mod native_plugin;
mod panel;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("launcher-simple");

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
