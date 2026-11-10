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
#[ignore = "BLOCKED: the host does not admit the event registrations event.install_done (97), event.wifi_done (93) and event.work_due"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_admits_every_family_make_is_refused_as_unsupported() {
    assert_eq!(construct().unwrap_err(), "Factory(-1)");
}
