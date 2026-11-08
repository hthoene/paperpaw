//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The host does not yet admit the navigation and event declaration families
//! (only screens are wired, #310), so it refuses this construction with
//! `Unsupported` (-1). The positive case is an explicit refusal until then;
//! the second test pins that it is that refusal, not a service-state error
//! from `make` itself.
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
        paperpaw_plugin_widgets::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "widgets",
        &["core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host admits screen declarations only; navigation/event batches are refused"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

/// Until the host wires the families the widget pack declares, `make` is
/// refused: `Unsupported` (-1) for the families not admitted yet, or
/// `NoSpace` (-3) where the declarations are staged as owned copies without
/// an image: the time family declares 131 settings and the host takes 128 per
/// plugin that way (`SPECS_MAX`, provisional). Declarations read from the
/// plugin's image have room for 512 entries, which is how the built-in pack
/// is loaded; a construction test with an image range is #944.
#[test]
fn until_the_host_admits_every_family_make_is_refused() {
    let refused = construct().unwrap_err();
    assert!(
        ["Factory(-1)", "Factory(-3)"].contains(&refused.as_str()),
        "{refused}: not a service-state refusal from make"
    );
}
