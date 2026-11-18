//! The plugin is constructed in the real registered scope of the kernel's
//! native host: `make` there has no live frame, so it only registers its
//! declarations (the services open in the callbacks that need them).
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
        paperpaw_plugin_lock::NATIVE_ENTRY.entry(),
        "lock",
        &["core.secrets", "core.ui"],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
