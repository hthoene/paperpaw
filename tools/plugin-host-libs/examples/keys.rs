//! The plugin `keys` as a shared library for the emulator's interface-7 loader.
#![no_std]
#![no_main]
use paperpaw_plugin_keys as _;

#[path = "../mem.rs"]
mod mem;
