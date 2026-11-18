//! The plugin `battery` as a shared library for the emulator's interface-7 loader.
#![no_std]
use paperpaw_plugin_battery as _;

#[path = "../mem.rs"]
mod mem;
