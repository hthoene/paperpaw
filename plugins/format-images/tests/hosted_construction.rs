//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` submits the format declaration, which the host checks
//! and admits (the format broker admits formats).
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
        paperpaw_plugin_format_images::NATIVE_ENTRY.entry(),
        "format-images",
        &[],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
