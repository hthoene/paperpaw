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

#[test]
fn make_runs_in_a_real_registered_scope() {
    hosted::try_construct(
        paperpaw_plugin_wifi::NATIVE_ENTRY.entry(),
        "wifi",
        &[
            "core.device",
            "core.net",
            "core.secrets",
            "core.ui",
            "core.work",
        ],
    )
    .expect("construction");
}
