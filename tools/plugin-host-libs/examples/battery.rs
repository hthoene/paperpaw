//! The plugin `battery` as a shared library for the emulator's interface-7 loader.
#![no_std]
#![no_main]
use paperpaw_plugin_battery as _;

#[path = "../mem.rs"]
mod mem;
