//! The plugin is constructed in the real registered scope of the kernel's
//! native host: `make` submits the format declaration, which the host checks
//! and admits, and the teardown is clean.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

#[test]
fn make_runs_in_a_real_registered_scope() {
    hosted::try_construct(
        paperpaw_plugin_format_cbz::NATIVE_ENTRY.entry(),
        "format-cbz",
        &[],
    )
    .expect("construction");
}
