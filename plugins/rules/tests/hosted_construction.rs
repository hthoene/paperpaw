//! The plugin is constructed in the real registered scope of the kernel's native host,
//! installed through `Kernel::register_linked`. `make` has no live frame, so it opens no
//! service: it submits its declarations and the host takes the whole batch, the nine event
//! registrations included (the per-plugin cap is 32 since #871).
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
        paperpaw_plugin_rules::NATIVE_ENTRY.entry(),
        "rules",
        &["core.registry", "core.ui"],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
