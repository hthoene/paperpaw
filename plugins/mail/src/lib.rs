//! Mail: read and send e-mail over IMAP and SMTP with a password or an app
//! password (no browser login).

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration tests also use the builders"
)]
mod declarations;
mod native_entry;
pub mod native_model;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 mail plugin (see `PORTING-7.md`).
pub mod native_plugin;
mod native_provider;
mod native_reader;
mod native_screens;
mod native_socket;
mod native_values;
mod native_widgets;
mod native_words;
mod native_writer;
