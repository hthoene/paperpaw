//! Pomodoro: a focus timer with work and break rounds.
//!
//! - One timer (phase, end time, rounds, the day's counts) is shared by every
//!   part and kept in a small file, so a round survives a restart.
//! - A work timer (`core.work`) ends the phase on time, whatever screen is open
//!   or none, says so with a notice and, as set, a flash of the screen or a
//!   blink of the front light, and may start the next phase by itself.
//! - *Pomodoro* (the Tools menu): a large countdown, a progress bar, the dots of
//!   the set and Start/Pause, Skip and Reset. It redraws once a minute (every ten
//!   seconds in the last one), only the block of the clock, never more.
//! - Widgets for the home screen (and every screen showing widgets): *Pomodoro
//!   timer* (phase and time left, a tap starts or pauses), *Pomodoro start/pause*
//!   (a button) and *Pomodoro today* (finished rounds, the daily goal, the
//!   week). They are drawn when the timer changes and each minute while it
//!   runs; none holds the device awake.
//!
//! Works with the settings as they come: 25, 5 and 15 minutes, four rounds.
//! Advanced: auto-start, the end signal and a daily goal.
//!
//! The plugin is a native interface-7 plugin (`native_plugin`); what it decides
//! without a host is in `phase`, `state` and `native_shown` (`PORTING-7.md`).

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

#[path = "../declarations.rs"]
pub mod declarations;
mod native_entry;
pub mod native_plugin;
mod native_screen;
pub mod native_shown;
mod native_widgets;
pub mod phase;
pub mod state;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

use paperpaw_sdk::Id;

pub const PLUGIN: Id = Id::new_static("pomodoro");

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
