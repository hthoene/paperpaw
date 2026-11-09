//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` submits the declarations, which the host checks and
//! then refuses as `Unsupported` (-1) until it admits every declaration family
//! (Core #944); the second test pins that it is that refusal.
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
        paperpaw_plugin_update::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "update",
        &["core.device", "core.files", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host does not yet admit every declaration family"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_admits_every_family_make_is_refused_as_unsupported() {
    assert_eq!(construct().unwrap_err(), "Factory(-1)");
}
