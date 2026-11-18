//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host admits an event registration only for the tags `events::is_event_tag`
//! names. This plugin registers event.screen_ready (87, admitted with #925) and
//! event.stack_changed (81, admitted with #964 and delivered by #977): the host
//! takes both, so the plugin is made.
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
fn make_is_admitted_with_both_events() {
    construct().unwrap();
}
