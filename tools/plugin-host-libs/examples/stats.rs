//! The plugin `stats` as a shared library for the emulator's interface-7 loader.
#![no_std]
use paperpaw_plugin_stats as _;

#[path = "../mem.rs"]
mod mem;
