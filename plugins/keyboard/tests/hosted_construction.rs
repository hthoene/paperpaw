//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The plugin declares a key binding (`key.bind`: layer `keyboard.screen`, input
//! `long.back`, action `keyboard.done`). The host checks it, but
//! `native_install::map_record` has no mapping for the record yet, so the whole
//! install is refused with `Unsupported` (-1) after `make` (`Status(-1)`). The
//! positive case stays ignored until the host maps `key.bind`; the second test
//! pins that refusal.
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
        paperpaw_plugin_keyboard::NATIVE_ENTRY.entry(),
        "keyboard",
        &["core.device", "core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED: the host install has no mapping for the key.bind record (keyboard.screen / long.back)"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn the_key_binding_is_refused_as_unsupported_until_the_host_maps_it() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Status(-1)",
        "`make` ran; the install refuses the key.bind record (`map_record`)"
    );
}
