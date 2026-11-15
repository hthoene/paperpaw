//! The language is switched on an open settings page: its rows say their
//! words in the new language at once, not only after the page is opened again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

/// The labels of what the screen shows.
fn labels(device: &mut Device) -> Vec<String> {
    let mut labels = Vec::new();
    device.call(|kernel, _| {
        labels = kernel.describe().into_iter().map(|p| p.label).collect();
        paperpaw_core::Effects::default()
    });
    labels
}

#[test]
fn a_settings_page_says_its_rows_in_the_language_chosen_at_once() {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", "[help]\nshow = false\n");
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    run_script(
        &mut device,
        "wait:3000,open:settings.group:id=core.group.system,wait:1500",
    )
    .unwrap();
    let before = labels(&mut device);
    assert!(
        before.iter().any(|l| l.starts_with("Language")),
        "no language row: {before:?}"
    );
    device.call(|kernel, now| kernel.set_setting(now, "core.language", "de").unwrap());
    run_script(&mut device, "wait:1500").unwrap();
    let after = labels(&mut device);
    assert!(
        after.iter().any(|l| l.starts_with("Sprache")),
        "the row still has its old words: {after:?}"
    );
    assert!(
        !after.iter().any(|l| l.starts_with("Language")),
        "{after:?}"
    );
}
