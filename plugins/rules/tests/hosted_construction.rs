//! The plugin is constructed in the real registered scope of the kernel's native host,
//! installed through `Kernel::register_linked`. `make` has no live frame, so it opens no
//! service. The host admits the event tags it registers (64, 65, 66, 67, 73) but refuses the
//! batch with `NoSpace` (-3) when `make` submits it; the second test pins that refusal, not a
//! service-state error from `make` itself. The positive case stays ignored until it passes.
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
#[ignore = "BLOCKED: the host admits the event tags now, but refuses this plugin's batch with ERR_NO_SPACE (-3) when `make` submits it"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_takes_the_whole_batch_make_is_refused_as_no_space() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Factory(-3)",
        "not a service-state refusal from make"
    );
}
