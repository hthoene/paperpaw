//! The plugin `reading_goal` as a shared library for the emulator's interface-7 loader.
#![no_std]
#![no_main]
use paperpaw_plugin_reading_goal as _;

#[path = "../mem.rs"]
mod mem;
