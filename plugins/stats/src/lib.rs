//! Reading statistics: how long, how much and on which days one reads,
//! with an overview, a calendar and every book's numbers.
//!
//! Recorded as KOReader records its statistics, so the two can exchange
//! them: every stay on a page with its start, duration and the book's
//! pages then; stays shorter than *Shortest page* (5 s) are left out and
//! longer ones count as *Longest page* (2 min), so a book left open does
//! not count as read. Books are recognised by KOReader's fingerprint of
//! the file (a partial MD5). Reading pauses while the app is in the
//! background and ends when the book is closed. Stays are timed on the
//! device's monotonic clock and dated by the wall clock: read before the
//! device knows the time (an X4 until Wi-Fi sets its clock), they wait
//! in memory and are recorded once it does. See `store` for the files.
//!
//! The statistics are shared with KOReader's: through the cloud folder
//! KOReader syncs them to (WebDAV), and by taking over those of KOReader
//! on the same device (see `sync`); its database format is in
//! `koreader`, the merge in `merge`.

#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_calendar;
mod native_chart;
mod native_entry;
mod native_left;
mod native_overview;
mod native_screens;
mod native_sync_screen;
mod native_sync_shell;
mod native_webdav;
mod native_webdav_net;
mod native_widget_calendar;
mod native_widget_ring;
mod native_widget_streak;
mod native_widget_today;
mod native_widget_week;
mod native_widget_year;
mod native_widgets;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/stats_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;

/// The setting groups the plugin declares, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_GROUPS: &[paperpaw_sdk::SettingGroupSpec] = &native_settings::GROUPS;

/// The full ids the plugin declares, sorted, as a table kept in the program
/// like [`NATIVE_SETTINGS`] (the kernel takes them in place of copies).
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;

mod book_list;
mod book_rows;
mod calendar_model;
mod extsort;
mod files;
mod koreader;
mod merge;
mod overview_rows;
mod recorder;
mod room;
mod store;
mod texts;
mod totals;

#[cfg(test)]
mod native_webdav_tests;
#[cfg(test)]
mod screens_tests;
#[cfg(test)]
mod testing;

use paperpaw_sdk::Id;
use paperpaw_sdk::setting::Setting;

pub const PLUGIN: Id = Id::new_static("stats");
/// Records reading.
pub const ENABLED: Setting<bool> = Setting::new("stats.enabled");
/// Stays on a page shorter than this many seconds are not counted.
pub const MIN_SECONDS: Setting<i64> = Setting::new("stats.min-seconds");
/// Stays on a page count at most this many seconds.
pub const MAX_SECONDS: Setting<i64> = Setting::new("stats.max-seconds");
/// Opens the overview.
pub const OPEN: Id = Id::new_static("stats.open");

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
