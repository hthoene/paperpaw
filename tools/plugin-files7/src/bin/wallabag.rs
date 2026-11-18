//! The plugin `wallabag` as a native plugin file of format 5 (see `../../README.md`).
//! The plugin crate's `native-entry` feature exports the entry and the
//! metadata (`paperpaw_sdk::native_plugin!`); nothing else is needed here.

#![no_std]
#![no_main]

use paperpaw_plugin_wallabag as _;
