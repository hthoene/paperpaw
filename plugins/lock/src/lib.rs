//! The screen lock: a PIN, a pattern or a password asked for when PaperPaw
//! starts and when the device wakes up, before anything else can be used. Off
//! by default. A native plugin of interface 7 (see `PORTING-7.md`).
//!
//! - Set up on the lock's own page (Settings › Device › *Screen lock*): the
//!   code in use is asked for first, then the kind (off, PIN, pattern,
//!   password) and the new code twice. Patterns are offered on touch screens.
//! - The code is kept as a salted hash among the passwords and keys, never as
//!   it is. Like those, it keeps the device from casual use, not from someone
//!   who takes its files.
//! - Locked, the lock screen takes every input; only going to sleep,
//!   refreshing the screen and night mode pass, so a locked device still
//!   sleeps. Closed while locked, it opens again at once.
//! - After five wrong tries, the next one waits half a minute; the count is
//!   kept on the card, so starting anew does not skip the wait.
//! - The kind is changed on the lock's own screen, which asks for the code in
//!   use first: changed elsewhere, it goes back to the kind whose code is kept.
//!   With no code kept any more, the lock screen lets the device be used.
//! - *Lock now* (`lock.now`) locks at once, from any button, gesture, menu or
//!   rule.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_clock;
mod native_entry;
#[doc(hidden)]
pub mod native_flow;
pub mod native_gate;
#[doc(hidden)]
pub mod native_input;
#[doc(hidden)]
pub mod native_model;
#[doc(hidden)]
pub mod native_pad;
/// The interface-7 screen lock.
pub mod native_plugin;
mod native_screen;
mod native_store;
/// The touch lock's way out.
#[doc(hidden)]
pub mod native_touch;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/lock_settings.rs"));
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
pub static NATIVE_TITLES: &[(paperpaw_sdk::Id, &str)] = &native_settings::TITLES;

#[cfg(test)]
mod native_parity_tests;
