//! Calibre's wireless device connection: while the screen is open, the
//! device finds Calibre in the local network (or connects to the address
//! set), and Calibre sends books to it, lists them and deletes them, as
//! with a reader on a cable. Books go into the library folder, where
//! Calibre places them (`Author/Title.epub`).
//!
//! In Calibre: *Connect/share*, *Start wireless device connection*. A
//! password set there is asked for once and kept with the other passwords.
//!
//! Calibre adds and deletes books here, so the device connects only to a
//! computer the reader chose: one found in the network is shown with its
//! address and trusted once the reader connects to it (the address set
//! by hand is trusted as it is). Calibre writes only books of formats
//! PaperPaw reads, below the library folder, replaces and deletes only
//! books it sent itself, and never touches the reader's own files.
//!
//! The protocol is in `native_model`, the session in `native_session`, the
//! books Calibre knows in `native_store`. The connection is a socket
//! (`core.sockets`); books come straight into a file of the plugin's data and
//! are put in the library, so their size does not matter.

// The heap word is assembly (`native_plugin!`); Xtensa still gates it.
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_entry;
#[doc(hidden)]
pub mod native_keep;
#[doc(hidden)]
pub mod native_model;
/// The wireless library, as a native plugin (see `PORTING-7.md`).
pub mod native_plugin;
#[doc(hidden)]
pub mod native_session;
mod native_store;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The plugin's id.
pub const PLUGIN: &str = "calibre";

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
