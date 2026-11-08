//! Rules: when something happens, PaperPaw does something. For example
//! "every day at 22:00: night mode on", "on weekdays at 7:00: sync the
//! catalogs", "when the battery falls below 20 %: the light off", "when
//! the battery reaches 80 %, only while charging: a message to unplug"
//! (where the device cannot stop charging by itself).
//!
//! - When: PaperPaw starts; at a time of day (every day, on weekdays, at
//!   weekends or on one day); every so many minutes; a book is opened,
//!   closed or read to its end; the device wakes up or goes to sleep; the
//!   battery falls below a percentage or reaches one (once, until it fell
//!   clearly below again); charging begins or ends; a switch
//!   (any on/off setting) is turned on or off.
//! - Then: run any action (from any plugin), turn any switch on or off,
//!   set a number (e.g. the light) or show a message.
//! - Only if (optional): between two times of day, while reading or not,
//!   on battery or while charging, while a switch is on or off.
//!
//! The rules are listed under Settings › Customise › Rules: tapping one
//! pauses it, holding it offers a condition or to remove it, *Add rule*
//! asks when and what. Times of day and rules that repeat follow the
//! host's minute tick. Rules set off by a setting another rule changed
//! follow each other at most [`MAX_CHAIN`](native_rules::engine::MAX_CHAIN)
//! deep: two rules that undo each other stop there. The rules are read once
//! and kept; where the file cannot be read, none apply and the list says so
//! instead of saving over it.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeRules`]; the
//! interface-6 plugin is gone.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_data;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;
/// The rules file and what the rules do, without the host.
pub mod native_rules;

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
