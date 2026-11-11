//! The plugin is constructed in the real registered scope of the kernel's
//! native host, installed through `Kernel::register_linked`: `make` submits its
//! declarations and the host admits them all.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

const SERVICES: &[&str] = &[
    "core.device",
    "core.document",
    "core.files",
    "core.library",
    "core.net",
    "core.registry",
    "core.secrets",
    "core.ui",
    "core.work",
];

fn construct() -> Result<(), String> {
    hosted::try_construct(
        paperpaw_plugin_transfer::NATIVE_ENTRY.entry(),
        "transfer",
        SERVICES,
    )
}

/// The host admits every family `make` submits (the screens with the phone's
/// and the cable's slot offers, the navigation and the events install_done,
/// wifi_done and work_due), and `core.library` for the cable's books.
#[test]
fn make_runs_in_a_real_registered_scope() {
    construct().expect("construction");
}

/// A shell that backs no `uses.install` (the desktop, the emulator) still
/// registers the plugin: installing plugins sent from the computer is then
/// refused by the host, and everything else works.
#[test]
fn make_registers_where_the_host_offers_no_install_feature() {
    hosted::try_construct_lacking(
        paperpaw_plugin_transfer::NATIVE_ENTRY.entry(),
        "transfer",
        SERVICES,
        &["uses.install"],
    )
    .expect("construction without uses.install");
}
