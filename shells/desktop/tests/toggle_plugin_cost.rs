//! Switching a plugin in Settings > Manage plugins (#375): on the X4 the
//! Confirm blocked the main loop for 16.5 s (the watchdog bites at 20 s)
//! because the settings page below the plugin manager was made anew twice,
//! each time with one registry call for every settings group. Only the plugin
//! manager's own list may be read; the settings page is made anew when it
//! shows again.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

fn focus(device: &mut Device, label: &str) {
    for _ in 0..120 {
        let parts = device.kernel.describe();
        if parts.iter().any(|p| p.focused && p.label.contains(label)) {
            return;
        }
        run_script(device, "right").unwrap();
    }
    panic!("no focus on {label}");
}

/// The service calls since the last `reset`, by plugin and operation, and
/// those of the settings plugin's group walk.
fn calls() -> (usize, usize) {
    let all = slot_counts::service_calls();
    let walks = all
        .iter()
        .filter(|(plugin, service, op)| {
            plugin == "settings" && *service == "core.registry" && *op == "groups"
        })
        .count();
    (all.len(), walks)
}

#[test]
fn switching_a_plugin_does_not_make_the_covered_settings_page_anew() {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[help]\nshow = false\n[reader]\nopen-last = false\n",
        );
    paperpaw_emulator::budget::override_limit(None);
    let mut device = Device::in_memory(
        profiles::find("xteink-x4").unwrap(),
        None,
        Rc::new(files),
        true,
        None,
    )
    .unwrap();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    run_script(&mut device, "open:settings.list").unwrap();
    run_script(&mut device, "wait:300").unwrap();
    focus(&mut device, "Manage plugins");
    run_script(&mut device, "confirm").unwrap();
    run_script(&mut device, "wait:1000").unwrap();
    focus(&mut device, "Transfer");
    slot_counts::reset();
    run_script(&mut device, "confirm").unwrap();
    run_script(&mut device, "wait:500").unwrap();
    let (total, walks) = calls();
    eprintln!("switching one plugin: {total} service calls, {walks} group reads");
    // Measured: 359 calls with 130 group reads (the settings page made anew
    // twice); 103 and none with the page left alone, the plugin manager's own
    // list and its places.
    assert_eq!(walks, 0, "the settings page below was made anew");
    assert!(total <= 140, "{total} service calls");
    // Back at the settings page, it is made anew once, as it shows.
    slot_counts::reset();
    run_script(&mut device, "back").unwrap();
    run_script(&mut device, "wait:500").unwrap();
    let (_, walks) = calls();
    assert!(
        walks > 0,
        "the settings page showed without being made anew"
    );
}
