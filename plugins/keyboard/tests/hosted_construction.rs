//! The plugin is constructed in the real registered scope of the kernel's
//! native host. `make` there has no live frame, so it must not open services
//! (it registers its declarations; `core.ui` opens in the first callback).
//!
//! The plugin brings hyphenation files, and a linked built-in has no folder
//! they could be read from, so the host refuses the whole install with
//! `Unsupported` (-1) before `make` runs (`Kernel::register_native_in`). The
//! positive case stays ignored until the host serves a built-in's files; the
//! second test pins that refusal.
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
#[ignore = "BLOCKED: the host refuses a linked built-in with hyphenation files (no folder)"]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

#[test]
fn a_linked_built_in_with_hyphenation_files_is_refused_as_unsupported() {
    let refused = construct().unwrap_err();
    assert_eq!(
        refused, "Status(-1)",
        "refused before `make` runs: font or hyphenation files need a folder"
    );
}
