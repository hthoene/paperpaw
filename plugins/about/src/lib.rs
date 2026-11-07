//! *About PaperPaw*, in the device's settings: a page as the other
//! settings pages. Its rows are the version, the address of the source,
//! the plugins built into the firmware, the interface and draft code
//! plugins must be built for, and the licences.
//!
//! The page is the interface-7 plugin [`native_plugin::NativeAbout`]; the
//! interface-6 page is gone. Two rows wait on the host and say so: the
//! licences and the plugins built in open their text in the reader, which
//! needs `doc_open.root` 4 (draft 21, `command.open` into `core.reader`);
//! and other plugins' lines (`about.lines`) come through a plugin-defined
//! stage once stages are on integration (#330). The licences text
//! (`data/licences-*.md`, `tools/licences/build.py`) and the logo stay in
//! the crate for that slice.

#![no_std]

extern crate alloc;

/// Where PaperPaw's source is.
pub const SOURCE: &str = "https://github.com/hthoene/paperpaw";

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
mod native_model;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;
