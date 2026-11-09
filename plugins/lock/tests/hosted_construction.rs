//! The plugin is constructed in the real registered scope of the kernel's
//! native host: `make` there has no live frame, so it only registers its
//! declarations (the services open in the callbacks that need them).
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
        paperpaw_plugin_lock::NATIVE_ENTRY.entry(),
        include_bytes!("../native/metadata.host.bin"),
        "lock",
        &["core.secrets", "core.ui"],
    )
}

#[test]
#[ignore = "BLOCKED: the harness loads without an image, so the host has no declaration views; see below"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

/// Every batch is admitted (a refused one ends `make` with `Factory(-1)`, as the
/// subscription to a changed setting did): what is left is that this harness
/// passes no image (`(null, 0)`), so the host cannot give views of what was
/// declared. Production loads pass the image.
#[test]
fn every_batch_is_admitted_and_only_the_missing_image_refuses() {
    let refused = construct().unwrap_err();
    assert_eq!(refused, "Lifecycle(DeclarationViewsUnavailable)");
}
