//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host admits every family the plugin declares: screens, actions,
//! settings, navigation, menus, slots and the event subscriptions
//! `event.setting_changed` (64), `event.plugins_changed` (70),
//! `event.registry_changed` (75) and `event.preview_changed` (76) (the last
//! two since #916), all through `Kernel::register_linked`.
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
        paperpaw_plugin_settings::NATIVE_ENTRY.entry(),
        "settings",
        &["core.registry", "core.secrets", "core.ui"],
    )
}

#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}
