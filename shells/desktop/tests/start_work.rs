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

/// The blocks the reader asks of the document service from a start into the
/// book read last to the first frame: those of the page on show (3 now; 56
/// when the script hint read the book's first 80 blocks before it, each a host
/// call of about 10 ms on the X4).
const START_BLOCKS: usize = 10;

#[test]
fn a_start_into_the_last_book_reads_only_its_page_and_writes_nothing() {
    let files = MemoryStorage::new()
        .with(
            "books/alice-in-wonderland.epub",
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").to_vec(),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            ".paperpaw/plugins/reader/state.ini",
            "[reader]\nlast = books/alice-in-wonderland.epub\n",
        )
        .with(
            ".paperpaw/positions/b1348f1e1a4d5649.txt",
            "[position]\nsection = 5\noffset = 13336\npercent = 28.7\n\
             conversion = af63bc4c8601b62c\n\
             xpointer = /body/DocFragment[6]/body/div[1]/p[38]/text()[1].910\n\
             book = 189249 59feb863c3a0652fe66a99c0978404f9\n",
        )
        .with(
            "paperpaw.ini",
            "[core]\nhyphenate = on\n[help]\nshow = false\n[reader]\nopen-last = true\n",
        );
    // English is hyphenated as on a fresh install (the firmware seeds the pack).
    let files = Rc::new(
        paperpaw_default_packs::english()
            .into_iter()
            .fold(files, |files, (path, bytes)| {
                files.with(&format!("plugins/hyphenation/{path}"), bytes)
            }),
    );
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    // The first start counts the book's pages and keeps what it found; the
    // second is the device's everyday start.
    {
        let mut first = Device::in_memory(profile, None, Rc::clone(&files), true, None).unwrap();
        first.start();
        paperpaw_emulator::headless::run_script(&mut first, "wait:20000").unwrap();
    }
    let mut device = Device::in_memory(profile, None, files, true, None).unwrap();
    slot_counts::reset();
    device.start();
    let counts = slot_counts::snapshot();
    let blocks = slot_counts::service_calls()
        .iter()
        .filter(|(plugin, service, op)| {
            plugin == "reader" && *service == "core.document" && *op == "block"
        })
        .count();
    let writes: u32 = counts
        .iter()
        .filter(|(slot, _)| ["storage_write", "storage_append"].contains(slot))
        .map(|(_, n)| *n)
        .sum();
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    eprintln!("start into the book: {blocks} blocks, {writes} writes");
    assert_eq!(screen.as_deref(), Some("reader.screen"));
    assert!(
        blocks <= START_BLOCKS,
        "the first frame asked {blocks} blocks (at most {START_BLOCKS})"
    );
    assert_eq!(writes, 0, "the first frame wrote to the card");
}
