//! The home editor's item options with the X4's buttons (#440, #450): the menu
//! that opens on an item (Move, Change the size, Settings, Remove; the bottom
//! menu of every other screen) takes the buttons: Left and Right move through it,
//! Select chooses, Back closes it.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::rc::Rc;

use paperpaw_core::sdk::{MemoryStorage, semantics};
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

fn device() -> Device {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", "[help]\nshow = false\n");
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
    device
}

fn focused(device: &mut Device, label: &str) -> bool {
    let parts = device.kernel.describe();
    semantics::find(&parts, label).is_some_and(|part| part.focused)
}

fn shown(device: &mut Device, label: &str) -> Option<String> {
    let parts = device.kernel.describe();
    semantics::find(&parts, label).map(|part| part.label.clone())
}

#[test]
fn the_buttons_move_through_the_options_and_choose() {
    let mut device = device();
    // The pencil, then the first item's options (the editor starts on it).
    run_script(&mut device, "wait:1500,right,right,confirm,wait:800").unwrap();
    run_script(&mut device, "confirm,wait:800").unwrap();
    assert!(focused(&mut device, "Move"));
    run_script(&mut device, "right,wait:500").unwrap();
    assert!(
        focused(&mut device, "Change the size"),
        "Right moves the focus"
    );
    run_script(&mut device, "left,wait:500").unwrap();
    assert!(focused(&mut device, "Move"), "Left moves it back");
    // Back closes the menu: it is gone.
    run_script(&mut device, "back,wait:500").unwrap();
    assert!(shown(&mut device, "Change the size").is_none());
    // Back on the page the first press shows the focus again, the next opens
    // the options: Select on an option runs it, sizing starts and the menu closes.
    run_script(
        &mut device,
        "confirm,wait:800,confirm,wait:800,right,wait:300,confirm,wait:800",
    )
    .unwrap();
    assert!(shown(&mut device, "Change the size").is_none());
}
