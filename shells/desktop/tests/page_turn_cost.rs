//! What a page turn costs in host crossings (#2936). On the X4 each crossing
//! from the reader into the host takes a millisecond or more, and 80 % of a
//! turn was spent inside them: the reader read its settings about 390 times
//! and crossed once per word in every band it was asked to draw. The turn
//! took 2.3 to 2.6 s, Beta 3's 0.3 s draw included.
//!
//! This counts the host slots a turn enters with the kernel's `slot-counts`
//! feature (the emulator counts like the device; its milliseconds are not the
//! device's, its call counts are). Ceilings are in `CEILINGS`; they sit above
//! what a reader that reads its setup once and draws each word once needs,
//! and far below what it did. One test only: the counts are per thread, and
//! this file is one binary.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};
use paperpaw_platform::{ButtonState, InputEvent, buttons};

const BOOK: &str = "books/alice-in-wonderland.epub";

/// Turns that are not measured: the book opens and its caches fill over the
/// first turns (the first four after opening read 162, 125, 22 and 113 times
/// from the card, against 13 to 59 later; on the device the first turn is far
/// dearer still).
const WARM_UP: usize = 8;
/// Turns measured; each must stay under every ceiling.
const MEASURED: usize = 30;

/// `(slot, most entries one turn may make, why)`, for the sample page at the
/// default size (the worst of 30 turns on the reader of #1488 in brackets).
///
/// A word, and every other piece of text on the page, is drawn once per turn,
/// not once per band as before (654 inked draws, 413 settings reads and 1,437
/// entries in all on integration f19c3494b). The reader reads its setup when
/// it changes, not at a turn. The rest guards a slot that grows unseen.
const CEILINGS: &[(&str, u32, &str)] = &[
    ("settings_read", 5, "read once and refreshed on change (0)"),
    (
        "text_draw_inked",
        300,
        "one per piece of text, not per band (261)",
    ),
    (
        "text_measure",
        50,
        "layout of the next page, widths kept for the book (41; 224 before)",
    ),
    (
        "storage_read_at",
        150,
        "card reads; a chapter's first page (87)",
    ),
    ("service_call", 30, "document service and status line (17)"),
];

/// All slots together, one turn (323 now).
const TOTAL_CEILING: u32 = 340;

/// Floors, so that a turn which does nothing cannot pass: every measured turn
/// enters at least this many slots (the quietest, a page with one picture,
/// made 100), and the turns together draw at least this many inked pieces of
/// text per turn on average (about 205 measured).
const TURN_FLOOR: u32 = 50;
const INKED_PER_TURN_FLOOR: u32 = 100;

fn press(device: &mut Device, state: ButtonState) {
    device.call(|kernel, now| {
        kernel.handle(
            now,
            InputEvent::Button {
                name: buttons::PAGE_FORWARD,
                state,
            },
        )
    });
    device.advance(400);
}

fn reading() -> Device {
    let files = MemoryStorage::new()
        .with(
            BOOK,
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").as_slice(),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        )
        .with("paperpaw.ini", "[help]\nshow = false\n");
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    paperpaw_emulator::headless::run_script(&mut device, "wait:3000").unwrap();
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    assert_eq!(screen.as_deref(), Some("reader.screen"));
    device
}

fn count_of(counts: &[(&'static str, u32)], slot: &str) -> u32 {
    counts
        .iter()
        .find(|(name, _)| *name == slot)
        .map_or(0, |(_, n)| *n)
}

#[test]
fn a_page_turn_stays_within_its_host_crossings() {
    let mut device = reading();
    for _ in 0..WARM_UP {
        press(&mut device, ButtonState::Pressed);
        press(&mut device, ButtonState::Released);
    }
    let mut worst = vec![0_u32; CEILINGS.len()];
    let mut total = 0_u32;
    let mut quietest = u32::MAX;
    let mut inked = 0_u32;
    for _ in 0..MEASURED {
        slot_counts::reset();
        press(&mut device, ButtonState::Pressed);
        press(&mut device, ButtonState::Released);
        let counts = slot_counts::snapshot();
        eprintln!(
            "page turn: measures {} reads {} service {} settings {} inked {}",
            count_of(&counts, "text_measure"),
            count_of(&counts, "storage_read_at"),
            count_of(&counts, "service_call"),
            count_of(&counts, "settings_read"),
            count_of(&counts, "text_draw_inked"),
        );
        let turn: u32 = counts.iter().map(|(_, n)| *n).sum();
        total = total.max(turn);
        quietest = quietest.min(turn);
        inked += count_of(&counts, "text_draw_inked");
        for (at, (slot, ..)) in CEILINGS.iter().enumerate() {
            worst[at] = worst[at].max(count_of(&counts, slot));
        }
    }
    let mut over = Vec::new();
    for ((slot, ceiling, why), seen) in CEILINGS.iter().zip(&worst) {
        eprintln!("page turn: {slot}: most {seen} (ceiling {ceiling}, {why})");
        if seen > ceiling {
            over.push(format!("{slot}: {seen} > {ceiling} ({why})"));
        }
    }
    eprintln!("page turn: all slots together: most {total} (ceiling {TOTAL_CEILING})");
    if total > TOTAL_CEILING {
        over.push(format!("all slots together: {total} > {TOTAL_CEILING}"));
    }
    if quietest < TURN_FLOOR {
        over.push(format!(
            "a turn made only {quietest} slot entries, under {TURN_FLOOR}: it did nothing"
        ));
    }
    if inked < INKED_PER_TURN_FLOOR * MEASURED as u32 {
        over.push(format!(
            "{inked} inked draws in {MEASURED} turns, under {} : the pages were not drawn",
            INKED_PER_TURN_FLOOR * MEASURED as u32
        ));
    }
    assert!(
        over.is_empty(),
        "a page turn crosses into the host too often: {over:#?}"
    );
}
