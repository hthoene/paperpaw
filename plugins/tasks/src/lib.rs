//! Tasks: to-do lists with due dates and priorities, on the reader alone or
//! in step with a CalDAV server.
//!
//! - *Tasks* (the Tools menu) shows a list: *Add a task* first, then the open
//!   tasks by their day and priority, the done ones last. Select ticks a task
//!   off; holding it (or Menu) offers its title, day, priority and removal.
//!   The first row chooses the list, or *Today and overdue* across all lists.
//! - Widgets for the home screen: *Tasks today*, *Task list*, *Open tasks* and
//!   *Next task*. A tap on a task ticks it off.
//! - Without an account the lists live in a file on the card. With one
//!   (*CalDAV account*: address, name, password) every list of tasks of the
//!   server is a list here, kept in step when the lists are opened, on *Sync
//!   now*, and when the network is there anyway. What is changed without a
//!   network waits and is sent later.
//!
//! The plugin is a native interface-7 plugin (`native_plugin`). What it
//! decides without a host is in `model` (the lists, their order, the file's
//! text), `keep` (the file, written whole or not at all), `sync` (the runs
//! against a server) and `when` (a typed day); the CalDAV and iCalendar code
//! itself is the shared crate `paperpaw-caldav` (`PORTING-7.md`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
pub mod declarations;
pub mod keep;
pub mod model;
mod native_asks;
mod native_entry;
pub mod native_plugin;
mod native_screens;
mod native_secret;
mod native_widgets;
pub mod sync;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod test_volume;
pub mod when;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("tasks");

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
