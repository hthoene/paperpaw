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

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
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
