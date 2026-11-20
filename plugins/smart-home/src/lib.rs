//! Smart home: values and switches from your own Home Assistant.
//!
//! - The entities you choose (sensors, lights, switches, scenes) are kept in a
//!   small file with their last state; the server is asked for them over its
//!   REST API (`/api/states`, the whole list saved to a file and read in
//!   pieces, only the chosen ones kept) and told to switch them
//!   (`/api/services/<domain>/<service>`).
//! - *Smart home* (the Tools menu): the dashboard, the entity picker (a search
//!   over the server's list) and the connection (address and long-lived
//!   token, a secret). Choosing a switch, light or scene switches it and says
//!   what the server answered.
//! - Widgets for the home screen: *Smart home value*, *group*, *switch* and
//!   *room*.
//! - Fetched when the dashboard opens, when you ask, at a wake only where Wi-Fi
//!   is up already, and on a schedule while charging; never by a radio of its
//!   own.
//!
//! The plugin is a native interface-7 plugin (`native_plugin`); what it decides
//! without a host is in `model`, `states` and `link` (`PORTING-7.md`). It works
//! with Home Assistant; the name says only what it shows.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod connect;
mod dash;
#[path = "../declarations.rs"]
pub mod declarations;
mod fetch;
mod keep;
pub mod link;
pub mod model;
mod native_entry;
pub mod native_plugin;
mod native_sleep;
mod native_widgets;
mod pick;
mod say;
pub mod states;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("smart-home");

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
