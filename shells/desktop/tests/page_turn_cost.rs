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

/// Turns that are not measured: the book opens and its layout is built on the
/// first turns (on the device the first turn after opening is far dearer).
const WARM_UP: usize = 3;
/// Turns measured; each must stay under every ceiling.
const MEASURED: usize = 12;

/// `(slot, most entries one turn may make, why)`.
///
/// A page of the sample at the default size holds about 117 words. A word is
/// drawn once per turn; a word that straddles two bands may be drawn twice,
/// and the page furniture adds a few. `settings_read`: the reader reads its
/// setup when it changes, not at each turn. The rest is a guard against a
/// slot that grows without anyone seeing it.
const CEILINGS: &[(&str, u32, &str)] = &[
    ("settings_read", 40, "read once and refreshed on change"),
    ("text_draw_inked", 200, "once per word, not once per band"),
    ("text_measure", 200, "once per word at most"),
    (
        "storage_read_at",
        200,
        "card reads; the worst turn of 12 made 162",
    ),
    (
        "service_call",
        50,
        "document service and status line; worst 28",
    ),
];

/// All slots together, one turn: 1,437 before the fixes, of which 413 were
/// settings and 654 inked words. What is left after them is about 600.
const TOTAL_CEILING: u32 = 800;

fn press(device: &mut Device, state: ButtonState) {
    device.call(|kernel, now| {
        let effects = kernel.handle(
            now,
            InputEvent::Button {
                name: buttons::PAGE_FORWARD,
                state,
            },
        );
        effects
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
    for _ in 0..MEASURED {
        slot_counts::reset();
        press(&mut device, ButtonState::Pressed);
        press(&mut device, ButtonState::Released);
        let counts = slot_counts::snapshot();
        total = total.max(counts.iter().map(|(_, n)| *n).sum());
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
    assert!(
        over.is_empty(),
        "a page turn crosses into the host too often: {over:#?}"
    );
}
