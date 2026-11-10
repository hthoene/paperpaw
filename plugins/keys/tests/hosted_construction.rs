//! The plugin is constructed in the real registered scope of the kernel's native host,
//! installed through `Kernel::register_linked`: `make` submits its declarations (it opens no
//! service: there is no live frame in `make`) and the host admits them all, the event
//! registrations among them (`event.registry_changed`, 75, since #916).
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
        paperpaw_plugin_keys::NATIVE_ENTRY.entry(),
        "keys",
        &["core.registry", "core.ui"],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
