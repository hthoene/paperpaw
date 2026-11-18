//! *How it works* of a linked plugin is reached with the X4's buttons alone
//! (#357): the plugin manager's list takes Select held (and the menu button)
//! for a hold, as a long touch is, and opens the plugin's text in the reader;
//! without it the host took the key for its menu.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};
use paperpaw_platform::Storage;

fn labels(device: &mut Device) -> Vec<String> {
    device
        .kernel
        .describe()
        .into_iter()
        .map(|part| part.label.to_string())
        .collect()
}

#[test]
fn buttons_reach_the_how_it_works_of_a_linked_plugin() {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[core]\nhyphenate = off\n[help]\nshow = false\n[reader]\nopen-last = false\n",
        );
    let files = Rc::new(files);
    let card = files.clone();
    paperpaw_emulator::budget::override_limit(None);
    let mut device = Device::in_memory(
        profiles::find("xteink-x4").unwrap(),
        None,
        files,
        true,
        None,
    )
    .unwrap();
    device.start();
    run_script(&mut device, "wait:3000,open:plugin-manager.list,wait:300").unwrap();
    // The X4 has no touch screen: the focus goes down the list with the buttons.
    let mut at = 0;
    while !device
        .kernel
        .describe()
        .iter()
        .any(|part| part.focused && part.label.starts_with("About"))
    {
        at += 1;
        assert!(
            at < 200,
            "the focus never reached About: {:?}",
            labels(&mut device)
        );
        run_script(&mut device, "right").unwrap();
    }
    run_script(&mut device, "long:confirm,wait:1500,describe").unwrap();
    // The reader is on top, and it was handed the plugin's name and text.
    let screen = device.kernel.active_screen().map(ToString::to_string);
    assert_eq!(
        screen.as_deref(),
        Some("reader.screen"),
        "{:?}",
        labels(&mut device)
    );
    let mut bytes = vec![0; 8192];
    let size = card
        .read_at(".paperpaw/plugins/plugin-manager/how.txt", 0, &mut bytes)
        .unwrap();
    let how = String::from_utf8(bytes[..size].to_vec()).unwrap();
    assert!(
        how.starts_with("About\n\nAbout shows which version"),
        "{how}"
    );
}

/// A hold only opens *How it works*: the plugin stays as it was, no switch is
/// staged by the press or by the release (#360).
#[test]
fn a_hold_does_not_switch_the_plugin() {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[core]\nhyphenate = off\n[help]\nshow = false\n[reader]\nopen-last = false\n",
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
    run_script(&mut device, "wait:3000,open:plugin-manager.list,wait:300").unwrap();
    let mut at = 0;
    while !device
        .kernel
        .describe()
        .iter()
        .any(|part| part.focused && part.label.starts_with("About"))
    {
        at += 1;
        assert!(at < 200, "the focus never reached About");
        run_script(&mut device, "right").unwrap();
    }
    let row = |device: &mut Device| {
        device
            .kernel
            .describe()
            .into_iter()
            .find(|part| part.label.starts_with("About"))
            .map(|part| part.label.to_string())
            .unwrap()
    };
    let before = row(&mut device);
    assert!(!before.contains("restart"), "{before}");
    run_script(&mut device, "long:confirm,wait:1500").unwrap();
    let screen = device.kernel.active_screen().map(ToString::to_string);
    assert_eq!(screen.as_deref(), Some("reader.screen"));
    run_script(&mut device, "back,wait:800").unwrap();
    assert_eq!(
        device
            .kernel
            .active_screen()
            .map(ToString::to_string)
            .as_deref(),
        Some("plugin-manager.list")
    );
    assert_eq!(row(&mut device), before);
    // A short press still switches it (staged until a restart).
    run_script(&mut device, "confirm,wait:800").unwrap();
    let after = row(&mut device);
    assert_ne!(after, before, "a short press must switch the row");
}
