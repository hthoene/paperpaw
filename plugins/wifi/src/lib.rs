//! Wi-Fi, where PaperPaw manages it itself (the X4): when the radio is on,
//! which network it joins, and the page to choose networks. An interface-7
//! native plugin only (ticket #244; see `PORTING-7.md`). Another plugin may
//! take its place: the shell only does what it is told, and the networks
//! joined are kept with the passwords and keys, the same for any plugin.
//!
//! By default the radio is off and comes on only when a plugin wants the
//! network (a catalog, a sync, a transfer), joins the network joined
//! before that the user picked last, or the strongest, and goes off again
//! a minute after nothing uses it, or at once when home or a book shows: it
//! costs battery and memory.

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
mod native_entry;
pub mod native_ini;
pub mod native_known;
#[doc(hidden)]
pub mod native_model;
pub mod native_own;
pub mod native_phone;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod native_policy;
pub mod native_radio;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
#[cfg(test)]
mod native_secret_tests;

use paperpaw_sdk::setting::Setting;

/// The radio stays on (in the network joined before) while charging: the
/// setting `wifi.while-charging`, whose default the X4's firmware sets.
pub const WHILE_CHARGING: Setting<bool> = Setting::new("wifi.while-charging");

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/wifi_settings.rs"));
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

/// The ids the plugin's declarations name, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;
