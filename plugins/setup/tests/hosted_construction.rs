//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host does not yet admit the navigation and event declaration families
//! (only screens are wired, #310), so it refuses this construction with
//! `Unsupported` (-1). The positive case is an explicit refusal until then;
//! the second test pins that it is that refusal, not a service-state error
//! from `make` itself.
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
        paperpaw_plugin_setup::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "setup",
        &["core.registry", "core.library", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host admits screen declarations only; navigation/event batches are refused"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_admits_every_family_make_is_refused_as_unsupported() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Factory(-1)",
        "not a service-state refusal from make"
    );
}
