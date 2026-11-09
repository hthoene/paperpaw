//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` and `core.net` open in the first
//! callback).
//!
//! `make` here runs to its end (every batch of screens, navigation and events
//! is submitted and staged); what the host refuses is the commit: it does not
//! wire declaration batches to its registry views yet
//! (`DeclarationViewsUnavailable`, #944). The positive case is an explicit
//! refusal until then; the second test pins that it is that refusal, not a
//! service-state error from `make` itself.
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
        paperpaw_plugin_browser::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "browser",
        &["core.net", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host does not wire declaration batches to its registry views yet"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_wires_the_registry_views_the_commit_is_refused() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Lifecycle(DeclarationViewsUnavailable)",
        "make ran to its end; the commit is what the host refuses"
    );
}
