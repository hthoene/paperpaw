//! First start on a fresh card: "Skip setup" in the setup assistant's dialog
//! leaves the assistant for Home and writes the `done` file (#433).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

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

fn fresh(profile: &str) -> (Device, Rc<MemoryStorage>) {
    let files = Rc::new(MemoryStorage::new().with(
        "paperpaw.ini",
        "[core]\nhyphenate = off\n[help]\nshow = false\n",
    ));
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find(profile).unwrap();
    let mut device = Device::in_memory(profile, None, files.clone(), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    (device, files)
}

#[test]
fn skip_setup_leaves_the_assistant_and_writes_done() {
    let (mut device, files) = fresh("xteink-x3");
    assert_eq!(screen(&mut device), "setup.guide");
    run_script(
        &mut device,
        "back,wait:1500,right,wait:500,confirm,wait:3000",
    )
    .unwrap();
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(files.exists(".paperpaw/plugins/setup/done"));
}
