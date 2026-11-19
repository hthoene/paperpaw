//! The work of an X4's start to the first home frame, in declaration records
//! (#403): the X4 runs its code and reads the linked plugins' declarations
//! from flash, so every record checked or read again costs the start time. A
//! record is checked once at its admission (it cannot change before its
//! commit and the stores after it), and a menu's rows find their icons and
//! submenus without reading every icon and menu row for each row.

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};

/// The records `validate_record` checks while the linked plugins are hosted
/// (3,522 now; 7,087 when every visit of the commit and the store's staging
/// checked them again).
const HOSTING_CHECKS: u32 = 4_000;

/// The records read back from the declaration store from the start to the
/// first home frame (1,762 now; 2,712 when each menu row read every icon and
/// every menu row again).
const FIRST_FRAME_READS: u32 = 2_000;

#[test]
fn the_start_to_the_first_home_frame_checks_and_reads_few_declarations() {
    let files = MemoryStorage::new()
        .with(
            "books/alice-in-wonderland.epub",
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").to_vec(),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[help]\nshow = false\n[reader]\nopen-last = false\n",
        );
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    slot_counts::reset();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    let (hosting_checks, _) = slot_counts::declarations();
    slot_counts::reset();
    device.start();
    let (_, first_frame_reads) = slot_counts::declarations();
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    eprintln!(
        "start: hosting checked {hosting_checks} records, the first frame read {first_frame_reads}"
    );
    assert_eq!(screen.as_deref(), Some("home.dashboard"));
    assert!(
        hosting_checks <= HOSTING_CHECKS,
        "hosting checked {hosting_checks} declaration records (at most {HOSTING_CHECKS})"
    );
    assert!(
        first_frame_reads <= FIRST_FRAME_READS,
        "the first home frame read {first_frame_reads} declaration records (at most {FIRST_FRAME_READS})"
    );
}
