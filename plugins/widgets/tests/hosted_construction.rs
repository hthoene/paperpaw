//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! Installed through `Kernel::register_linked` without an image range (a linked
//! built-in has none), the host stages the declarations as owned copies, which
//! the full pack exceeds (see the second test). The positive case stays ignored
//! until a pack can be installed from an image range.
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
        "widgets",
        &["core.files", "core.library", "core.registry", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED: without an image the host stages owned copies, 128 settings / 32 KiB; the pack declares more"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

/// Without an image (`(null, 0)`, as here) the host stages the declarations
/// as owned copies: 128 settings and 32 KiB per plugin, and the time family
/// declares 131. Production loads pass the image (the flash mapping on the
/// X4, `.rodata` on the desktop), which has room for 512 entries; the full
/// pack constructing waits for an install from an image range.
#[test]
fn without_an_image_make_is_refused_for_the_owned_copy_limit() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Factory(-3)",
        "owned-copy limit without an image: SPECS_MAX 128 / 32 KiB; production loads pass the image"
    );
}
