//! The widget pack on interface 7: many more widgets for the home screen (and
//! any screen that shows widgets), each with settings of its own and the shared
//! look of the SDK's widgets (its frame, size and alignment): clocks of several
//! kinds, a calendar, the year so far, quotes, notes, to-dos, pictures, covers,
//! progress, what to read next, highlights.
//!
//! The parts share one catalog a language (`i18n/<language>.ini`), as one
//! namespace.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod native_analogue;
mod native_battery;
mod native_battery_shapes;
mod native_block;
mod native_books;
mod native_calendar;
mod native_countdown;
mod native_current;
mod native_dates;
mod native_day;
mod native_defs;
mod native_draw;
mod native_entry;
mod native_favourites;
mod native_greeting;
mod native_highlight;
mod native_logo;
mod native_moon;
mod native_next;
mod native_note;
mod native_numbers;
mod native_picture;
mod native_picture_host;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_progress;
mod native_quote;
mod native_reading;
mod native_recorder;
mod native_series;
mod native_shelf;
mod native_tiles;
mod native_todo;
mod native_todo_list;
#[cfg(test)]
mod native_todo_list_tests;
mod native_todo_screen;
mod native_word_clock;
mod native_world;
mod native_year;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The native declarations (widgets, settings, events) of the plugin.
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and tests read the same declarations"
)]
mod declarations;

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
