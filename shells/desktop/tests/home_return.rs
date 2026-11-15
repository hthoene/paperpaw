//! Back to Home (#267): the home screen keeps the page it laid out while
//! another screen covers it, and on return lays it out again only where what it
//! shows could have changed. On the X4 the lay-out (settings, registry, every
//! widget made) took 2.7 s of a 4.4 s return. This counts the host slots of the
//! return with the kernel's `slot-counts` feature (the emulator counts like the
//! device; its milliseconds are not the device's, its call counts are).
//! One binary, counts are per thread: the tests do not share a thread.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

const BOOK: &str = "books/alice-in-wonderland.epub";

/// The host slots a return may enter, all together and the settings reads
/// (emulator, X4, Home with its default page). Measured with the page kept: 966
/// slots and 423 reads; laid out again on every return, as before: 1,349 and
/// 712 (a draw alone is about 640 and 270; the return draws twice, grays on).
const KEPT_ALL_SLOTS: u32 = 1050;
const KEPT_SETTINGS: u32 = 470;
/// A return that lays the page out again reads at least this many settings
/// (614 measured, with a grid of three columns).
const LAID_SETTINGS: u32 = 550;

/// An X4 at the home screen (or in the book, `last`), the help hint off.
fn device(last: bool) -> Device {
    let mut files = MemoryStorage::new()
        .with(
            BOOK,
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").as_slice(),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", "[help]\nshow = false\n");
    if last {
        files = files.with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        );
    }
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    device
}

fn screen(device: &mut Device) -> String {
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    screen.unwrap_or_default()
}

fn shot(device: &mut Device, name: &str) -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("home-return-{}-{name}.png", std::process::id()));
    paperpaw_emulator::headless::screenshot(device, &path).unwrap();
    std::fs::read(&path).unwrap()
}

/// The host slots entered by `steps`, all together, and the settings reads.
fn counted(device: &mut Device, steps: &[&str]) -> (u32, u32) {
    slot_counts::reset();
    for step in steps {
        run_script(device, step).unwrap();
    }
    let counts = slot_counts::snapshot();
    let at = |slot: &str| {
        counts
            .iter()
            .find(|(name, _)| *name == slot)
            .map_or(0, |(_, n)| *n)
    };
    eprintln!("home return: {counts:?}");
    (counts.iter().map(|(_, n)| *n).sum(), at("settings_read"))
}

/// Home, then the drawer of all tools over it, then back: the host slots of
/// the back.
fn back_from_the_drawer(device: &mut Device) -> (u32, u32) {
    assert_eq!(screen(device), "home.dashboard");
    run_script(device, "run:home.all-tools").unwrap();
    run_script(device, "wait:2000").unwrap();
    assert_eq!(screen(device), "home.drawer");
    counted(device, &["back", "wait:3000"])
}

#[test]
fn the_home_screen_keeps_its_page_while_another_screen_covers_it() {
    let mut device = device(false);
    let (all, settings) = back_from_the_drawer(&mut device);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        all <= KEPT_ALL_SLOTS && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
}

#[test]
fn a_setting_changed_under_the_cover_is_laid_out_when_it_returns() {
    let mut device = device(false);
    let before = shot(&mut device, "before");
    run_script(&mut device, "run:home.all-tools").unwrap();
    run_script(&mut device, "wait:2000").unwrap();
    device.call(|kernel, now| kernel.set_setting(now, "home.columns", "3").unwrap());
    run_script(&mut device, "wait:500").unwrap();
    let (all, settings) = counted(&mut device, &["back", "wait:3000"]);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        settings > LAID_SETTINGS,
        "the page was not laid out again: {all} slots, {settings} settings reads"
    );
    assert_ne!(
        shot(&mut device, "after"),
        before,
        "the new grid does not show"
    );
}

#[test]
fn what_a_book_changed_under_the_cover_shows_when_it_returns() {
    let mut device = device(true);
    // The first return lays the page out (the book was open from the start).
    run_script(&mut device, "back").unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    assert_eq!(screen(&mut device), "home.dashboard");
    let before = shot(&mut device, "card-before");
    // Back into the book, a number of pages on, and out again.
    run_script(
        &mut device,
        "open:reader.screen:path=books/alice-in-wonderland.epub",
    )
    .unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    assert_eq!(screen(&mut device), "reader.screen");
    for _ in 0..60 {
        run_script(&mut device, "page-forward").unwrap();
        run_script(&mut device, "wait:300").unwrap();
    }
    let heap = paperpaw_emulator::budget::stats();
    eprintln!("HEAP reader open: live {} peak {}", heap.live, heap.peak);
    let (all, settings) = counted(&mut device, &["back", "wait:3000"]);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        all <= KEPT_ALL_SLOTS && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
    assert_ne!(
        shot(&mut device, "card-after"),
        before,
        "the card shows the old progress"
    );
}

#[test]
fn a_plain_setting_changed_just_before_back_shows_when_it_returns() {
    let mut device = device(false);
    let before = shot(&mut device, "plain-before");
    run_script(&mut device, "run:home.all-tools").unwrap();
    run_script(&mut device, "wait:2000").unwrap();
    // Not the grid's, and no wait between the change and the Back: the event
    // reaches Home in the call that closes the cover.
    device.call(|kernel, now| {
        kernel
            .set_setting(now, "home.footer.pencil", "false")
            .unwrap()
    });
    run_script(&mut device, "back").unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    assert_eq!(screen(&mut device), "home.dashboard");
    assert_ne!(
        shot(&mut device, "plain-after"),
        before,
        "the footer still shows the pencil"
    );
}

#[test]
fn a_return_from_a_book_read_for_minutes_keeps_the_page() {
    let mut device = device(true);
    // The clock is set, so that Home's clock and card change at each minute.
    device.set_time(1_760_000_000_000);
    // The first return lays the page out (the book was open from the start).
    run_script(&mut device, "back").unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    run_script(
        &mut device,
        "open:reader.screen:path=books/alice-in-wonderland.epub",
    )
    .unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    assert_eq!(screen(&mut device), "reader.screen");
    // Minutes pass under the book: Home's own clock and card are drawn from what
    // they read (they have no widget instance of the host's, so no
    // `widget_changed`) and are no reason to lay the page out again.
    for _ in 0..8 {
        run_script(&mut device, "wait:15000,page-forward").unwrap();
    }
    let (all, settings) = counted(&mut device, &["back", "wait:3000"]);
    assert_eq!(screen(&mut device), "home.dashboard");
    // A return that lays the page out again reads over 550 settings (614 and
    // 1,298 slots measured); the widgets of the new minute draw a few slots
    // more than the first return does.
    assert!(
        all <= KEPT_ALL_SLOTS + 100 && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
}
