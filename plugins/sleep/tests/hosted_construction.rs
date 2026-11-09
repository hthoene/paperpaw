//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` submits the declarations, which the host checks and
//! then refuses as `Unsupported` (-1) until it admits every declaration family
//! and every event it subscribes to (Core #944); the second test pins that it
//! is that refusal.
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
        paperpaw_plugin_sleep::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "sleep",
        &["core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host does not yet admit every declaration family"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_admits_every_family_make_is_refused_as_unsupported() {
    // The subscription to `event.setting.changed` (the slot binding) names an
    // event the host has no encoder for yet: the batch is refused as
    // `Unsupported` (-1) at `make`, as for `home`; before that subscription
    // `make` ran to its end and the commit was refused (#944).
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Factory(-1)",
        "not a service-state refusal from make"
    );
}
