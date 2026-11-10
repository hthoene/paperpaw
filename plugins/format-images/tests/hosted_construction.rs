//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` submits the format declaration, which the host checks
//! and then refuses as `Unsupported` (-1) until its format broker admits
//! formats (Core #944); the second test pins that it is that refusal.
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
        paperpaw_plugin_format_images::NATIVE_ENTRY.entry(),
        "format-images",
        &[],
    )
}

#[test]
#[ignore = "BLOCKED #944: the host's format broker does not admit format declarations yet"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn until_the_host_admits_formats_make_is_refused_as_unsupported() {
    assert_eq!(construct().unwrap_err(), "Factory(-1)");
}
