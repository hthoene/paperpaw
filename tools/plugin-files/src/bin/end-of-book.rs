//! The plugin `end-of-book` as a native plugin file (see `../../README.md`).

#![no_std]
#![no_main]

paperpaw_sdk::plugin!(paperpaw_plugin_end_of_book::EndOfBook);
