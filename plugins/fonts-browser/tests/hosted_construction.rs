//! The plugin is constructed in the real registered scope of the kernel's
//! native host.
//!
//! The host admits every family `make` submits, event.install_done (97) among them
//! (admitted since f21484b09), so the plugin is made and torn down cleanly.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

fn construct() -> Result<(), String> {
    hosted::try_construct(
        paperpaw_plugin_fonts_browser::NATIVE_ENTRY.entry(),
        "fonts-browser",
        &["core.device", "core.net", "core.registry", "core.ui"],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
