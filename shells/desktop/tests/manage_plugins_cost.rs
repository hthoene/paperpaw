//! Settings > Manage plugins opens fast (#291): on the X4 the first frame took
//! 7 to 8 s, about 3 ms per host slot and 8 ms per card operation. This counts
//! the host slots between the open and the first frame with the kernel's
//! `slot-counts` feature (the emulator's milliseconds are not the device's, its
//! call counts are).
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

#[test]
fn the_first_frame_of_manage_plugins_stays_within_its_host_crossings() {
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
    slot_counts::reset();
    device.shown.clear();
    run_script(&mut device, "open:plugin-manager.list").unwrap();
    run_script(&mut device, "wait:300").unwrap();
    let counts = slot_counts::snapshot();
    let total: u32 = counts.iter().map(|(_, n)| *n).sum();
    eprintln!("manage plugins: total {total} {counts:?}");
    assert!(!device.shown.is_empty(), "the screen drew nothing");
    let measures = counts
        .iter()
        .find(|(slot, _)| *slot == "text_measure")
        .map_or(0, |(_, n)| *n);
    // Measured: 6,211 slots with 4,304 measures (every band measured the notes
    // of all rows, one word at a time); 1,829 and 279 with the heights kept,
    // one measure for a note and the rows outside a band not laid out; 1,016
    // with the stippled separators drawn in one slot each (836 fills before).
    assert!(measures <= 330, "{measures} text measures: {counts:?}");
    assert!(total <= 1050, "{total} host slots: {counts:?}");
}
