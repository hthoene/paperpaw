//! Calendar: your calendars on the reader, with agenda, day, week and month
//! views, home screen widgets and a sleep screen.
//!
//! - *Calendar* (the Tools menu) shows the agenda: today and the next days.
//!   The first row chooses the view (agenda, day, week, month); select on an
//!   event shows it whole.
//! - Sources are CalDAV accounts, calendars only read from an address
//!   (`.ics`, `webcal://`), calendar files on the card, and CardDAV address
//!   books for birthdays and anniversaries. Events repeat by their rules, with
//!   exceptions, all day or by the hour, in the zone they were made in.
//! - Widgets for the home screen: *Today*, *Next event*, *Agenda*, *Month* and
//!   *Birthdays*. The agenda is also a sleep screen.
//! - Calendars are read when the calendar is opened, on *Sync now*, and, as
//!   set, when the network is on anyway or when charging begins: never a radio
//!   switched on for it. Only events within sixty days of today are kept.
//!
//! The plugin is a native interface-7 plugin (`native_plugin`). What it
//! decides without a host is in `model` (the events and their repeats, the
//! file's text), `keep` (the file, written whole or not at all) and `sync`
//! (the runs against the sources); the CalDAV, iCalendar and vCard code is
//! the shared crate `paperpaw-caldav` (`PORTING-7.md`).

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
mod native_month;
pub mod native_plugin;
mod native_screens;
mod native_sleep;
mod native_text;
mod native_widgets;
pub mod sync;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod test_volume;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("calendar");

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
