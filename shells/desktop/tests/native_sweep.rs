//! A sweep over every plugin linked into the firmware
//! (`paperpaw_app::NATIVE_PLUGINS`), on the desktop shell's real wiring: the
//! device is built and started as the emulator starts it (every service the
//! shell wires, every plugin registered through `Kernel::register_linked`),
//! settled, and then the first screen of each plugin is opened in turn. One
//! test, one report: every plugin the host refused, every callback it dropped
//! (a service call refused, such as a reply buffer smaller than the op's
//! `max_reply`, is one), every kernel warning, and every first screen that did
//! not open. What is found is listed whole, so activation bugs of this kind
//! are found at once and not one by one in the emulator.
#![cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the test"
)]

use paperpaw_emulator::{device::Device, linked, profiles};
use paperpaw_platform::{ButtonState, InputEvent, buttons};
use paperpaw_sdk::{Id, MemoryStorage, Params};
use std::rc::Rc;

/// Runs simulated time on for `seconds`, a tick each second.
fn settle(device: &mut Device, seconds: u32) {
    for _ in 0..seconds {
        device.advance(1_000);
        device.tick_if_due();
    }
}

/// The plugins that have no screen of their own; any other without one is a
/// finding (its screen failed to declare).
const WITHOUT_SCREEN: [&str; 1] = ["power"];

/// Presses back, so the stack does not grow across the plugins and hide later
/// findings behind a cap.
fn back(device: &mut Device) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        device.input(InputEvent::Button {
            name: buttons::BACK,
            state,
        });
    }
    settle(device, 1);
}

/// What was reported since the last look: the host's lines about the linked
/// plugins and the kernel's warnings, each with `phase` in front.
fn took(device: &mut Device, phase: &str, findings: &mut Vec<String>) {
    findings.extend(
        linked::take_reports()
            .into_iter()
            .map(|line| format!("{phase}: {line}")),
    );
    findings.extend(
        device
            .warnings
            .drain(..)
            .map(|warning| format!("{phase}: kernel warned: {}", warning.message)),
    );
}

// Known causes still open (see #243): the plugins' catalogs (Core's linked
// catalogs fix) and `about` (Port C), whose open callback is refused as unsupported, probably
// for a device fact the desktop shell does not answer. Run it with `--ignored`; it must go green
// before native plugins are on by default.
#[test]
#[ignore = "#243: needs the linked catalogs (Core: core/linked-catalogs-228) and about (Port C); run with --ignored"]
fn every_linked_plugin_starts_and_its_first_screen_opens_without_a_failure() {
    let files = Rc::new(
        MemoryStorage::new()
            .with(
                "books/alice-in-wonderland.txt",
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            // The first-run setup was done.
            .with(".paperpaw/plugins/setup/done", ""),
    );
    let profile = profiles::find("xteink-x4").expect("the X4's profile");
    linked::take_reports();
    let mut device = Device::in_memory(
        profile,
        Some("[core]\nlanguage = en\n".to_string()),
        files,
        true,
        None,
    )
    .expect("the device");
    let mut findings = Vec::new();
    device.start();
    settle(&mut device, 5);
    took(&mut device, "start", &mut findings);
    let screens: Vec<Id> = device.kernel.known_screens().cloned().collect();
    for (plugin, _) in &paperpaw_app::NATIVE_PLUGINS {
        let Some(screen) = screens.iter().find(|id| id.namespace() == *plugin) else {
            if !WITHOUT_SCREEN.contains(plugin) {
                findings.push(format!("{plugin}: declared no screen"));
            }
            continue;
        };
        let phase = format!("{plugin}: opening {screen}");
        device.open_screen(screen.clone(), Params::new());
        settle(&mut device, 2);
        if device.kernel.active_screen() != Some(screen) {
            findings.push(format!(
                "{phase}: did not open (top: {:?})",
                device.kernel.active_screen().map(Id::as_str)
            ));
        }
        back(&mut device);
        took(&mut device, &phase, &mut findings);
    }
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}
