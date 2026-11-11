//! Fonts from the internet: a catalog of fonts (paperpaw.org's by
//! default), each name shown in its own font; one is shown larger with a
//! sentence in it, downloaded as a data plugin and read with at once.
//!
//! Nothing of a font is built in: devices that draw scalable fonts get
//! its font files, the others (the X4) font packs made for them, read a
//! glyph at a time from the storage. *More fonts* is offered where the
//! reading font and the status lines' font are chosen, and *Your own
//! fonts*, which shows how to read with a font file of one's own.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
pub mod native_catalog;
mod native_entry;
pub mod native_job;
pub mod native_plan;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_sample;
mod native_view;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod tests;
