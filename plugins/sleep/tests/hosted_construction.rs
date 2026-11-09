//! The plugin is constructed in the real registered scope of the kernel's
//! native host.
//!
//! The host admits an event registration only for the tags `events::is_event_tag`
//! names. This plugin registers event.plugins_changed (70), event.setting_changed (64), which it does not,
//! so the host refuses the batch with `Unsupported` (-1) when `make` submits it.
//! The positive case stays ignored until the host encodes those events; the
//! second test pins that it is that refusal, not a service-state error from
//! `make` itself. Every other family the plugin declares is admitted through
//! `Kernel::register_linked`.
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
        "sleep",
        &["core.files", "core.library", "core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED: the host does not admit the event registrations event.plugins_changed (70), event.setting_changed (64)"]
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
