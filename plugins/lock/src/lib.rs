//! The screen lock: a PIN, a pattern or a password asked for when
//! PaperPaw starts and when the device wakes up, before anything else can
//! be used. Off by default.
//!
//! - Set up under Settings › System › *Screen lock*: the code in use is
//!   asked for first, then the kind (off, PIN, pattern, password) and the
//!   new code twice. Patterns are offered on touch screens.
//! - The code is kept as a salted hash among the passwords and keys,
//!   never as it is. Like those, it keeps the device from casual use, not
//!   from someone who takes its files.
//! - Locked, the lock screen takes every input; only going to sleep,
//!   refreshing the screen and night mode pass, so a locked device still
//!   sleeps. Closed while locked, it opens again at once.
//! - After five wrong tries, the next one waits half a minute; the count
//!   is kept on the card, so starting anew does not skip the wait.
//! - *Lock now* (`lock.now`) locks at once, from any button, gesture,
//!   menu or rule.
//! - The touch lock: taps and swipes do nothing while it is on, the
//!   buttons still work, e.g. to read with a hand on the screen.
//!
//! The interface-7 plugin is built from the parts here; the interface-6 plugin
//! is gone.

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

/// The code, the wrong tries and the steps of unlocking and setting up.
pub mod native_core;
/// The keypad and the pattern: geometry and what is entered.
pub mod native_pad;
/// The touch lock's way out.
pub mod native_touch;

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
