//! The plugin is constructed in the real registered scope of the kernel's
//! native host.
//!
//! The host admits an event registration only for the tags `events::is_event_tag`
//! names. This plugin also registers event.install_done (97), which it does not,
//! so the host refuses the batch with `Unsupported` (-1) when `make` submits it.
//! The positive case stays ignored until the host encodes that event; the second
//! test pins that it is that refusal, not a service-state error from `make`
//! itself. Every other family the plugin declares is admitted through
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
        paperpaw_plugin_fonts_browser::NATIVE_ENTRY.entry(),
        "fonts-browser",
        &["core.device", "core.net", "core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED: the host does not admit the event registration event.install_done (97)"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_admits_every_family_make_is_refused_as_unsupported() {
    assert_eq!(construct().unwrap_err(), "Factory(-1)");
}
