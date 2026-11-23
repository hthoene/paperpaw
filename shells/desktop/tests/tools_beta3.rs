//! Wi-Fi, the transfer and the built-in tools as in Beta 3 (#481), walked on
//! the X4's buttons in German: the focus stays where an action left it, the
//! key bar names only the buttons that do something, a long text shows whole,
//! the password's keyboard offers the phone, the device check draws.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

/// An X4 in German, set up, with `config` added to `paperpaw.ini`.
fn x4(config: &str) -> Device {
    let ini = format!("[core]\nlanguage = de\n[help]\nshow = false\n{config}");
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

/// What the screen shows: each part's label, whether it has the focus, and
/// whether it is on the key bar (the lowest line of parts, along the bottom
/// of the panel).
fn parts(device: &mut Device) -> Vec<(String, bool, bool)> {
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
                (p.label, p.focused, on_bar)
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

/// The labels of the parts with the focus.
fn focused(device: &mut Device) -> Vec<String> {
    parts(device)
        .into_iter()
        .filter_map(|(label, focus, _)| focus.then_some(label))
        .collect()
}

/// The words on the key bar, left to right.
fn key_bar(device: &mut Device) -> Vec<String> {
    parts(device)
        .into_iter()
        .filter_map(|(label, _, bar)| (bar && !label.is_empty()).then_some(label))
        .collect()
}

fn labels(device: &mut Device) -> Vec<String> {
    parts(device).into_iter().map(|(label, ..)| label).collect()
}

/// Whether a part with the focus starts with `start`.
fn focus_on(device: &mut Device, start: &str) -> bool {
    focused(device).iter().any(|label| label.starts_with(start))
}

#[test]
fn the_focus_stays_on_the_network_joined_and_comes_back_to_networks() {
    let mut device = x4("");
    // Wi-Fi on, a scan, then the list of networks from its row.
    run_script(
        &mut device,
        "run:wifi.open,wait:500,confirm,wait:3000,right,right,wait:300",
    )
    .unwrap();
    assert!(
        focus_on(&mut device, "Netzwerke"),
        "{:?}",
        focused(&mut device)
    );
    run_script(&mut device, "confirm,wait:1000,right,wait:300").unwrap();
    assert_eq!(screen(&mut device), "wifi.networks");
    assert!(focus_on(&mut device, "Cafe"), "{:?}", focused(&mut device));
    // Joined (an open network): the list says so, the focus stays on it.
    run_script(&mut device, "confirm,wait:3000").unwrap();
    assert!(
        focus_on(&mut device, "Cafe, Verbunden"),
        "{:?}",
        focused(&mut device)
    );
    // Back: the page's focus is on the row the list was opened from.
    run_script(&mut device, "back,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "wifi.page");
    assert!(
        focus_on(&mut device, "Netzwerke"),
        "{:?}",
        focused(&mut device)
    );
}

#[test]
fn a_password_can_be_typed_on_the_phone() {
    let mut device = x4("");
    run_script(
        &mut device,
        "run:wifi.open,wait:500,confirm,wait:3000,right,right,confirm,wait:1000,confirm,wait:1000",
    )
    .unwrap();
    assert_eq!(screen(&mut device), "keyboard.screen");
    assert!(
        labels(&mut device)
            .iter()
            .any(|l| l.starts_with("Passwort für")),
        "{:?}",
        labels(&mut device)
    );
    // *Phone* in the field, with the focus to start with.
    assert_eq!(
        focused(&mut device).last().map(String::as_str),
        Some("Handy")
    );
    run_script(&mut device, "confirm,wait:1500").unwrap();
    assert_eq!(screen(&mut device), "transfer.phone");
}

#[test]
fn the_transfer_names_only_back_and_without_a_router() {
    let mut device = x4("");
    run_script(&mut device, "run:transfer.open,wait:2000").unwrap();
    assert_eq!(screen(&mut device), "transfer.screen");
    assert_eq!(key_bar(&mut device), ["Zurück", "Ohne Router"]);
    // The page of the own network: only Back does something.
    run_script(&mut device, "confirm,wait:2000").unwrap();
    assert!(
        labels(&mut device)
            .iter()
            .any(|l| l.starts_with("1. Mit dem Handy")),
        "{:?}",
        labels(&mut device)
    );
    assert_eq!(key_bar(&mut device), ["Zurück"]);
}

#[test]
fn the_battery_page_names_only_back() {
    let mut device = x4("[plugins]\nbattery = on\n");
    run_script(&mut device, "run:battery.open,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "battery.screen");
    assert_eq!(key_bar(&mut device), ["Zurück"]);
}

#[test]
fn the_device_check_draws_its_first_step() {
    let mut device = x4("[plugins]\ndiagnostics = on\n");
    run_script(&mut device, "run:diagnostics.open-check,wait:2000").unwrap();
    assert_eq!(screen(&mut device), "diagnostics.check");
    let shown = labels(&mut device);
    assert!(shown.iter().any(|l| l == "Gerätecheck"), "{shown:?}");
    // The first step, the screen's, with its words.
    assert!(shown.iter().any(|l| l == "Bildschirm"), "{shown:?}");
}

/// Whether `shown` holds `text` whole, its lines joined, nothing cut.
fn whole(shown: &[String], text: &str) -> bool {
    !shown.iter().any(|l| l.ends_with('…')) && shown.join(" ").contains(text)
}

#[test]
fn the_help_of_the_koreader_sync_shows_whole() {
    let mut device = x4("[plugins]\nstats = on\n");
    run_script(&mut device, "run:stats.open,wait:1000").unwrap();
    // The third row: *Sync with KOReader*.
    run_script(&mut device, "right,right,confirm,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "stats.sync");
    let shown = labels(&mut device);
    assert!(
        whole(&shown, "Dann führen beide Apps ihre Statistik zusammen."),
        "{shown:?}"
    );
}

#[test]
fn the_empty_pages_of_passwords_and_of_read_books_say_it_whole() {
    let mut device = x4("[plugins]\nstats = on\n");
    run_script(&mut device, "run:settings.secrets.open,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "settings.secrets");
    let shown = labels(&mut device);
    assert!(whole(&shown, "sobald sie eingegeben sind."), "{shown:?}");
    // The statistics' books, none read yet.
    run_script(
        &mut device,
        "back,run:stats.open,wait:1000,right,confirm,wait:1000",
    )
    .unwrap();
    assert_eq!(screen(&mut device), "stats.books");
    let shown = labels(&mut device);
    assert!(whole(&shown, "die du liest."), "{shown:?}");
}
