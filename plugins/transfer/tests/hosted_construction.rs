//! The plugin is constructed in the real registered scope of the kernel's
//! native host, installed through `Kernel::register_linked`: `make` submits its
//! declarations and the host admits them all.
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
        paperpaw_plugin_transfer::NATIVE_ENTRY.entry(),
        "transfer",
        &[
            "core.device",
            "core.document",
            "core.files",
            "core.net",
            "core.registry",
            "core.secrets",
            "core.ui",
            "core.work",
        ],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
