//! The widget pack: many more widgets for the home screen (and any screen
//! that shows widgets), each with settings of its own and the shared look
//! of `paperpaw_sdk::widgets` (its frame, size and alignment):
//!
//! - time: clocks of several kinds, a calendar, the year so far (`time`);
//! - words and pictures: quotes, notes, to-dos, pictures (`words`);
//! - books: covers, progress, what to read next, highlights (`books`).
//!
//! The parts share one catalog a language (`i18n/<language>.ini`), as one
//! namespace.

#![no_std]

extern crate alloc;

mod books;
mod native_calendar;
mod native_cx;
mod native_dates;
mod native_day;
mod native_defs;
mod native_draw;
mod native_entry;
mod native_names;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_tiles;
mod showings;
mod time;
mod words;

#[cfg(test)]
mod draw_tests;
#[cfg(test)]
mod harness;

use alloc::borrow::Cow;

use paperpaw_sdk::{Env, Id, Manifest, Plugin, RegisterError, Registrar};

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The native declarations (widgets, settings, events) of the plugin.
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and tests read the same declarations"
)]
mod declarations;

pub const PLUGIN: Id = Id::new_static("widgets");

/// The widget pack.
#[derive(Debug, Default, Clone, Copy)]
pub struct Widgets;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Widgets {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Widget pack", paperpaw_sdk::package_version!()).description(
                "Many more widgets for the home screen: clocks, a calendar, quotes, books, notes and more.",
            ).how(&HOW).uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        time::register(r)?;
        words::register(r)?;
        books::register(r)?;
        Ok(())
    }
}

/// The height `id`'s frame takes (the SDK's).
pub(crate) fn frame_height(env: &Env<'_>, id: &str) -> u16 {
    paperpaw_sdk::widgets::frame_height(env, id)
}

/// Today as days since 1970 in the local time zone; 0 where the device
/// does not know the date.
pub(crate) fn today(env: &Env<'_>) -> i64 {
    use paperpaw_sdk::time::{local_seconds, unix_seconds};
    unix_seconds(env).map_or(0, |unix| local_seconds(env, unix).div_euclid(86_400))
}

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
