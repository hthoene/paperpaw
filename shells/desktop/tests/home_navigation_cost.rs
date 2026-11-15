//! Home focus moves skip widgets outside the damaged bands (#267).
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};
use paperpaw_platform::{ButtonState, InputEvent};
use std::rc::Rc;

fn press(device: &mut Device, name: &'static str) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        device.input(InputEvent::Button { name, state });
        device.advance(80);
    }
}

#[test]
fn home_focus_moves_stay_within_their_host_crossings() {
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[help]\nshow=false\n[reader]\nopen-last=false\n",
        );
    let mut device = Device::in_memory(
        profiles::find("xteink-x4").unwrap(),
        None,
        Rc::new(files),
        true,
        None,
    )
    .unwrap();
    device.start();
    assert_eq!(
        device
            .kernel
            .active_screen()
            .map(ToString::to_string)
            .as_deref(),
        Some("home.dashboard")
    );
    for name in ["right", "left", "right", "left"] {
        slot_counts::reset();
        device.shown.clear();
        press(&mut device, name);
        let counts = slot_counts::snapshot();
        let setting_reads = counts
            .iter()
            .find(|(slot, _)| *slot == "settings_read")
            .map_or(0, |(_, n)| *n);
        let total: u32 = counts.iter().map(|(_, n)| *n).sum();
        assert!(
            !device.shown.is_empty(),
            "must measure a drawn selection move"
        );
        assert!(
            setting_reads <= 256,
            "{name}: {setting_reads} settings reads"
        );
        assert!(total <= 700, "{name}: {total} host crossings");
    }
}
