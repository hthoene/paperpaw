//! One focus move on Home (#308): only what the two widgets the focus leaves
//! and comes to draw again is drawn. A move asks the library nothing, reads
//! nothing from the card and draws a cover only where a band holds it: the
//! card's cover and the shelf's two, each in the band it lies in, not in every
//! band of the frame the two areas make. Counted with the kernel's
//! `slot-counts` feature (the emulator counts as the device; its milliseconds
//! are not the device's, its call counts are).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

const ALICE: &str = "books/alice-in-wonderland.epub";
const OTHER: &str = "books/die-verwandlung.epub";

fn count(counts: &[(&'static str, u32)], slot: &str) -> u32 {
    counts
        .iter()
        .find(|(name, _)| *name == slot)
        .map_or(0, |(_, n)| *n)
}

/// An X4 at Home after Alice was read last and Kafka before her: the card
/// shows Alice, the shelf Kafka.
fn home() -> Device {
    let files = Rc::new(
        MemoryStorage::new()
            .with(
                ALICE,
                include_bytes!("../../../samples/library/books/alice-in-wonderland.epub")
                    .as_slice(),
            )
            .with(
                OTHER,
                include_bytes!("../../../samples/library/books/die-verwandlung.epub").as_slice(),
            )
            .with(".paperpaw/plugins/setup/done", "")
            .with(
                "paperpaw.ini",
                "[core]\nhyphenate = off\n[help]\nshow = false\n",
            ),
    );
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    for book in [OTHER, ALICE] {
        files
            .write(
                ".paperpaw/plugins/reader/state.ini",
                format!("[reader]\nlast = {book}\n").as_bytes(),
            )
            .unwrap();
        let mut device = Device::in_memory(profile, None, files.clone(), true, None).unwrap();
        device.start();
        run_script(&mut device, "wait:3000,long:back,wait:6000").unwrap();
        if book == ALICE {
            return device;
        }
    }
    unreachable!()
}

/// What one move may enter (emulator, X4, the card, one book on the shelf):
/// measured 93 host slots and two covers; 146 and three before a band drew only
/// what lies in it, and every move asked the library for its books again.
const MOVE_SLOTS: u32 = 110;

#[test]
fn a_focus_move_on_home_asks_nothing_and_draws_each_cover_once() {
    let mut device = home();
    // The first press shows the focus; the moves are the ones after it, close
    // together (a rest of thirty seconds draws the page in grays first).
    run_script(&mut device, "right,wait:1500").unwrap();
    for step in ["right", "right", "right", "left", "left", "left", "left"] {
        slot_counts::reset();
        run_script(&mut device, step).unwrap();
        run_script(&mut device, "wait:300").unwrap();
        let counts = slot_counts::snapshot();
        let calls = slot_counts::service_calls();
        let slots: u32 = counts.iter().map(|(_, n)| *n).sum();
        assert!(
            calls
                .iter()
                .all(|(_, service, _)| *service != "core.library"),
            "{step}: the library was asked: {calls:?}"
        );
        assert_eq!(count(&counts, "storage_read_at"), 0, "{step}: card reads");
        assert!(
            count(&counts, "image_draw") <= 2,
            "{step}: {} image draws",
            count(&counts, "image_draw")
        );
        assert!(
            slots <= MOVE_SLOTS,
            "{step}: {slots} host slots: {counts:?}"
        );
    }
}

/// A minute that comes while Home is shown draws what changed in it (the clock
/// at most), not the whole page laid out anew: on the X4 that was a 1.8 s
/// rebuild of 480x755 a few seconds after a move, a key press waiting for it
/// (#308).
#[test]
fn a_minute_does_not_draw_the_page_again() {
    let mut device = home();
    device.shown.clear();
    run_script(&mut device, "right,wait:1500").unwrap();
    for _ in 0..8 {
        run_script(&mut device, "wait:20000").unwrap();
    }
    run_script(&mut device, "right,wait:1500").unwrap();
    let wide: Vec<_> = device
        .shown
        .iter()
        .filter(|(rect, mode)| {
            *mode != paperpaw_platform::RefreshMode::Full
                && u32::from(rect.width) * u32::from(rect.height) * 2 > 480 * 755
                && rect.x == 0
        })
        .collect();
    assert!(wide.is_empty(), "the page was drawn again: {wide:?}");
}
