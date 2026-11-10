//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host admits an event registration only for the tags `events::is_event_tag`
//! names. This plugin registers event.screen_ready (87, admitted with #925) and
//! event.stack_changed (81). Until the host admits event 81 (Core, #964), `make`
//! is refused with `Unsupported` (-1) when it submits the batch; once it does, the
//! plugin is made. The test takes either, so it holds in any merge order, and
//! pins that a refusal is that one, not a service-state error from `make` itself.
//! It does not register event.screen_shown (38): the host sends that to the token
//! of the screen declaration (`docs/abi/screens-input.md` section 2). Every other
//! family the plugin declares (screens, actions, settings, navigation, menus,
//! slots) is admitted through `Kernel::register_linked`.
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
fn make_runs_or_is_refused_only_for_event_81() {
    match construct() {
        Ok(()) => {}
        Err(refused) => assert_eq!(
            refused, "Factory(-1)",
            "not a service-state refusal from make"
        ),
    }
}
