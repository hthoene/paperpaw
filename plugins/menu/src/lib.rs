//! The menu overlay, an interface-7 native plugin.
//!
//! Shows the menu named by the `menu` parameter at the bottom of the screen
//! (`core.menu`, `native_plugin`), or as a panel of controls (`core.panel`,
//! `native_panel_screen`), over whatever is open. The rows come from the
//! registry: every plugin's entries or the user's own definition. Choosing an
//! action closes the menu and runs it; settings change in place.

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
mod native_data;
mod native_entry;
pub mod native_model;
pub mod native_panel;
pub mod native_panel_screen;
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
