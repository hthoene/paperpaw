//! The screen a device sleeps or switches off on is refreshed once, fully,
//! as Beta 3 did: the sleep screen asked for its full refresh only once it had
//! shown, which refreshed the whole screen a second time before the sleep
//! (#523).

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::{MemoryStorage, RefreshMode};
use paperpaw_emulator::{device::Device, headless, profiles};

/// An X4 on the home screen, its sleep screen `show` (a `sleep.show` value).
fn started(show: &str) -> Device {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            format!("[core]\nhyphenate = off\n[help]\nshow = false\n[sleep]\nshow = {show}\n"),
        );
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    headless::pass_time(&mut device, 2000);
    device
}

/// The frames shown while `steps` (buttons and how long each is held) put
/// the device to sleep or switched it off, and some time after.
fn frames_to_sleep(device: &mut Device, steps: &[(&str, u64)]) -> Vec<(u16, u16, RefreshMode)> {
    let before = device.frames_shown;
    for &(button, ms) in steps {
        headless::press_button(device, button, ms).unwrap();
        headless::pass_time(device, 500);
    }
    headless::pass_time(device, 3000);
    assert!(device.asleep(), "asleep");
    let new = usize::try_from(device.frames_shown - before).unwrap();
    device.shown[device.shown.len() - new..]
        .iter()
        .map(|(area, mode)| (area.width, area.height, *mode))
        .collect()
}

#[test]
fn switching_off_and_sleeping_refresh_the_screen_once() {
    let off = [("confirm", 60)];
    for show in ["sleep.show.cat", "sleep.show.widgets"] {
        // The whole screen once: in grays for the picture, fully for the widgets.
        let once = if show == "sleep.show.cat" {
            RefreshMode::Gray
        } else {
            RefreshMode::Full
        };
        let mut device = started(show);
        // The power menu (power held), then its second row: *Switch off*.
        headless::press_button(&mut device, "power", 1000).unwrap();
        headless::pass_time(&mut device, 500);
        headless::press_button(&mut device, "page-forward", 60).unwrap();
        headless::pass_time(&mut device, 500);
        let frames = frames_to_sleep(&mut device, &off);
        assert_eq!(frames, [(480, 800, once)], "{show}: switched off");
        // The power button: its press marks the corner first (as in Beta 3),
        // then the sleep screen.
        let mut device = started(show);
        let mut frames = frames_to_sleep(&mut device, &[("power", 60)]);
        frames.retain(|&(width, height, _)| (width, height) == (480, 800));
        assert_eq!(frames, [(480, 800, once)], "{show}: asleep");
    }
}
