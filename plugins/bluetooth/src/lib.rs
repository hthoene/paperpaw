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

pub use declarations::{BY_HAND, REMOTES, WHILE_AWAKE, WHILE_READING};
