//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host now admits every declaration this plugin makes, and refuses the
//! construction at the commit instead (`DeclarationViewsUnavailable`: wire
//! declarations cannot activate before the registry views are wired, #944).
//! The positive case is an explicit refusal until then; the second test pins
//! that it is that refusal, not a service-state error from `make` itself.
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
        paperpaw_plugin_profiles::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "profiles",
        &["core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host admits the declarations but cannot activate them before the registry views are wired"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_views_are_wired_make_is_refused_at_the_commit() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Lifecycle(DeclarationViewsUnavailable)",
        "not a service-state refusal from make"
    );
}
