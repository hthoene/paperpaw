//! The transfer's screens on the X4's buttons in German (#486): the phone's
//! typing screen without a network names only Back and offers no own network
//! (it is the transfer's, not the keyboard's), the cable's screen names only
//! Back, and the cable's permission and the reader's own network left alone
//! end while their screens stay and say so (a timer cannot leave a screen).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

/// An X4 in German, set up.
fn x4() -> Device {
    let ini = "[core]\nlanguage = de\n[help]\nshow = false\n";
    let files = MemoryStorage::new()
        .with("paperpaw.ini", ini)
        .with(".paperpaw/plugins/setup/done", "");
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:2000").unwrap();
    device
}

/// What the screen shows: each part's label, and whether it is on the key bar
/// (the lowest line of parts, along the bottom of the panel).
fn parts(device: &mut Device) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    device.call(|kernel, _| {
        let bottom = kernel.device().display.height;
        let described = kernel.describe();
        let bar = described
            .iter()
            .filter(|p| p.area.y.saturating_add(p.area.height) >= bottom.saturating_sub(2))
            .map(|p| p.area.y)
            .max();
        parts = described
            .into_iter()
            .map(|p| {
                let on_bar = Some(p.area.y) == bar;
                (p.label, on_bar)
            })
            .collect();
        paperpaw_core::Effects::default()
    });
    parts
}

fn screen(device: &mut Device) -> String {
    let mut screen = String::new();
    device.call(|kernel, _| {
        screen = kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default();
        paperpaw_core::Effects::default()
    });
    screen
}

/// The words on the key bar, left to right.
fn key_bar(device: &mut Device) -> Vec<String> {
    parts(device)
        .into_iter()
        .filter_map(|(label, bar)| (bar && !label.is_empty()).then_some(label))
        .collect()
}

/// Whether the screen says `text` (its parts' labels joined: a text wraps).
fn says(device: &mut Device, text: &str) -> bool {
    let labels: Vec<String> = parts(device).into_iter().map(|(label, _)| label).collect();
    labels.join(" ").contains(text)
}

#[test]
fn the_phone_screen_without_a_network_names_only_back() {
    let mut device = x4();
    // A password on the phone, the device in no network: the keyboard's
    // *Phone* opens the transfer's typing screen.
    run_script(
        &mut device,
        "run:wifi.open,wait:500,confirm,wait:3000,right,right,confirm,wait:1000,confirm,wait:1000,confirm,wait:2000",
    )
    .unwrap();
    assert_eq!(screen(&mut device), "transfer.phone");
    assert!(
        says(&mut device, "Zum Tippen auf dem Handy"),
        "{:?}",
        parts(&mut device)
    );
    // Before, it showed the transfer's *Without a router*, which did nothing
    // here, and named it on the key bar.
    assert!(
        !says(&mut device, "Ohne Router"),
        "{:?}",
        parts(&mut device)
    );
    assert_eq!(key_bar(&mut device), ["Zurück"]);
}

#[test]
fn the_cable_screen_names_only_back() {
    let mut device = x4();
    device.cable_line("PPT/1 1 GET /api/hello 0");
    run_script(&mut device, "wait:1000").unwrap();
    assert_eq!(screen(&mut device), "transfer.cable");
    // Before, Select and the arrows were named, which do nothing here.
    assert_eq!(key_bar(&mut device), ["Zurück"]);
}

#[test]
fn the_cable_left_alone_ends_the_permission_and_its_screen_says_so() {
    let mut device = x4();
    device.cable_line("PPT/1 1 GET /api/hello 0");
    run_script(&mut device, "wait:1000").unwrap();
    assert_eq!(screen(&mut device), "transfer.cable");
    // Five minutes without a request end the permission (the screen promises
    // it): before, the timer asked to close the screen, which the host
    // refuses there, and the permission stayed.
    run_script(&mut device, "wait:331000").unwrap();
    assert_eq!(screen(&mut device), "transfer.cable");
    assert!(
        says(&mut device, "Es ist nichts mehr erlaubt"),
        "{:?}",
        parts(&mut device)
    );
}

#[test]
fn the_own_network_left_alone_closes_and_its_screen_says_so() {
    let mut device = x4();
    run_script(&mut device, "run:transfer.open,wait:2000,confirm,wait:3000").unwrap();
    assert!(
        says(&mut device, "1. Mit dem Handy"),
        "{:?}",
        parts(&mut device)
    );
    // Ten minutes without a request: before, the timer asked to close the
    // screen, which the host refuses there, and the network stayed open.
    run_script(&mut device, "wait:601000").unwrap();
    assert_eq!(screen(&mut device), "transfer.screen");
    assert!(
        says(&mut device, "eigene Netzwerk des Readers ist geschlossen"),
        "{:?}",
        parts(&mut device)
    );
    assert!(!says(&mut device, "1. Mit dem Handy"));
    assert_eq!(key_bar(&mut device), ["Zurück"]);
}
