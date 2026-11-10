//! Comic books for PaperPaw, on interface 7: the plugin declares its format and
//! answers the host's `core.format` calls over the shared conversion of
//! [`paperpaw_epub_family`] (see `PORTING-7.md`).

#![no_std]

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
/// The interface-7 plugin.
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
