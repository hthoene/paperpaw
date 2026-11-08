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

/// Without an image (`(null, 0)`, as here) the host stages the declarations
/// as owned copies: 128 settings and 32 KiB per plugin, and the time family
/// declares 131. Production loads pass the image (the flash mapping on the
/// X4, `.rodata` on the desktop), which has room for 512 entries; the full
/// pack constructing from an image range is #944.
#[test]
fn without_an_image_make_is_refused_for_the_owned_copy_limit() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Factory(-3)",
        "owned-copy limit without an image: SPECS_MAX 128 / 32 KiB; production loads pass the image (#944)"
    );
}
