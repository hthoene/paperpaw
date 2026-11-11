//! The plugin manager: lists every plugin with its name, version and
//! description, and switches plugins on and off.
//!
//! - Each row shows a plugin and its state: *On*, *Off*, *Always on* (the
//!   kernel), *Required* (a plugin another one needs, or this manager:
//!   without it plugins could not be switched on again) or *Error*.
//! - Holding a row shows how the plugin works, where it says.
//! - Choosing a row switches the plugin. The choice applies after a
//!   restart: a row *Restart now* then comes last.
//! - Below each name: what happens to the plugin (if anything), its
//!   description and version. Plugins with native code say so, and why one
//!   does not run.
//! - After a start in safe mode, or with plugins the shell did not
//!   install, a notice says so.
//! - Above the plugins: *Screens* (which plugin shows each place several
//!   offer a screen for) and where more plugins come from, e.g. the store
//!   (the menu [`MORE`]): a row each, which opens it.
//!
//! The plugin is the interface-7 plugin [`native_plugin::NativePluginManager`];
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
/// The interface-7 plugin.
pub mod native_plugin;

pub use declarations::MORE;

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::SettingSpec;
    include!(concat!(env!("OUT_DIR"), "/plugin_manager_settings.rs"));
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
