//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host admits an event registration only for the tags `events::is_event_tag`
//! names. This plugin registers event.screen_shown (38), event.screen_ready (87), which it does not,
//! so the host refuses the batch with `Unsupported` (-1) when `make` submits it.
//! Event 38 (`event.screen_shown`) can never be registered as an event: the host
//! sends it to the token of the screen declaration (`docs/abi/screens-input.md`
//! section 2), so help must drop that subscription (Port A todo 331).
//! The positive case stays ignored until the host encodes the other events; the
//! second test pins that it is that refusal, not a service-state error from
//! `make` itself. Every other family the plugin declares (screens, actions,
//! settings, navigation, menus, slots) is admitted through `Kernel::register_linked`.
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
        paperpaw_plugin_help::NATIVE_ENTRY.entry(),
        "help",
        &["core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED: the host does not admit the event registrations event.screen_shown (38), event.screen_ready (87); event 38 can never be registered (screens-input.md section 2), help must drop it (Port A todo 331)"]
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
