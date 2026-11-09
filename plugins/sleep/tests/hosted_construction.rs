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
fn until_the_host_admits_every_family_the_commit_is_refused() {
    // `make` runs to its end; the host then refuses the commit of the declarations
    // (screen views, #944).
    assert_eq!(
        construct().unwrap_err(),
        "Lifecycle(DeclarationViewsUnavailable)"
    );
}
