//! Weather: the weather now and the next days as widgets for the home
//! screen (or any screen showing widgets), from Open-Meteo
//! (<https://open-meteo.com>, free and without a key).
//!
//! - *Weather now* (`now`): a symbol, the temperature, the condition,
//!   today's highest and lowest, and more to choose (wind, rain, ...).
//! - *Forecast* (`forecast`): the next days in a row, or as a list in
//!   tall places.
//!
//! Nothing is sent anywhere until the reader sets a place (by its name,
//! found with Open-Meteo's search, see `native_place`); then only its
//! coordinates. The weather is fetched while a weather widget is placed
//! on a page, at most each hour or as set, and only while there is a
//! network: where PaperPaw manages Wi-Fi it does not switch the radio on
//! for it, but waits until it is on. A tap on a widget fetches it at once.
//! The last answer is kept in a small file, so the widgets show it at once
//! after a start, with its age when old.
//!
//! The plugin is a native interface-7 plugin only (`native_plugin`); what it
//! decides without a host is in `native_weather`, `native_answers` and
//! `native_fetch` (`PORTING-7.md`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
pub mod declarations;
pub mod native_answers;
mod native_draw;
mod native_entry;
pub mod native_fetch;
mod native_forecast;
mod native_now;
mod native_place;
pub mod native_plugin;
pub mod native_shown;
mod native_symbol;
pub mod native_weather;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;
#[cfg(test)]
mod native_answers_tests;
#[cfg(test)]
mod native_golden_tests;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("weather");

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
