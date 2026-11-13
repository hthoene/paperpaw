//! Bluetooth, where the kernel uses a radio (the X4 Pro; see
//! `docs/abi/device.md`): the page of the devices bonded with this one,
//! adding one (a search, then pairing, which the kernel asks the reader to
//! confirm), forgetting one and giving a sensor to a plugin that uses
//! Bluetooth; and when a remote's or a keyboard's keys go to the key maps
//! (Remotes: by hand, while reading or while awake).
//!
//! Another plugin may take its place: the kernel keeps the bonds, with the
//! passwords and keys, the same for any plugin.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativeBluetooth`];
//! the interface-6 plugin is gone. What waits on the host and the SDK is in
//! `PORTING-7.md` and in `tests/acceptance_pending.rs`.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
#[doc(hidden)]
pub mod native_model;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/bluetooth_settings.rs"));
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
/// The interface-7 plugin.
pub mod native_plugin;

pub use declarations::{BY_HAND, REMOTES, WHILE_AWAKE, WHILE_READING};
