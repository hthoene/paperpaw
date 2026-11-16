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
use std::sync::{Mutex, MutexGuard};

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

/// The emulator's heap counter is one for the process: a test that reads it
/// (the repeated returns) must not run beside one that holds a device. Every
/// test holds this for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const BOOK: &str = "books/alice-in-wonderland.epub";

/// The host slots a return may enter, all together and the settings reads
/// (emulator, X4, Home with its default page). Measured with the page and its
/// look kept: 517-528 slots and 23-36 reads; with the page kept and its look
/// read at every draw: 857-868 and 333-346 (967-978 and 423-436 before the
/// names and widgets kept theirs); laid out again on every return, as before:
/// 1,349 and 712.
const KEPT_ALL_SLOTS: u32 = 560;
const KEPT_SETTINGS: u32 = 60;
/// The one step "a cover became ready" after the return: the library announces
/// the card's cover and Home draws that cell, 0.5 to 1.5 s after the first frame
/// (356 slots, 9 settings reads measured with the black-and-white first frame
/// of #1569; nothing else happens up to 7 s). The return itself is
/// `KEPT_ALL_SLOTS`; a laid-out page costs over 1,300.
const COVER_READY_SLOTS: u32 = 360;
/// A full redraw (five bands) of a page whose look is kept: slots, settings reads
/// (622 and 270 before the look was kept; 330 and about 12 measured).
const STEADY_SLOTS: u32 = 370;
const STEADY_READS: u32 = 40;
/// The first redraw after a setting changed reads the look of the page and of
/// each of its widgets again: 160 reads measured, 142 where the page forgets
/// nothing, 36 where its widgets forget nothing (18 in a steady redraw).
const CHANGED_READS: u32 = 150;
/// A return that lays the page out again reads at least this many settings
/// (614 measured, with a grid of three columns).
const LAID_SETTINGS: u32 = 120;

/// German, and a page of five columns with tools whose names are long enough
/// to reach the hyphen pass (`Einstellungen`).
const GERMAN_TOOLS: &str = "[core]\nlanguage = de\n[home]\ncolumns = 5\n\
    page-1 = tool:core.library 0/0 1x1, tool:core.settings 1/0 1x1, tool:store.open 2/0 1x1\n";

/// An X4 at the home screen (or in the book, `last`), the help hint off.
fn device(last: bool) -> Device {
    device_in(last, "")
}

/// As [`device`], with `ini` before the help hint's lines in `paperpaw.ini`.
fn device_in(last: bool, ini: &str) -> Device {
    let mut device = unstarted(last, ini);
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    device
}

/// As [`device_in`], before its start.
fn unstarted(last: bool, ini: &str) -> Device {
    let mut files = MemoryStorage::new()
        .with(
            BOOK,
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").as_slice(),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", format!("{ini}[help]\nshow = false\n"));
    if last {
        files = files.with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        );
    }
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    Device::in_memory(profile, None, Rc::new(files), true, None).unwrap()
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
    let _serial = serial();
    let mut device = device(false);
    let (all, settings) = back_from_the_drawer(&mut device);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        all <= KEPT_ALL_SLOTS && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
}

/// German with only the English hyphenation table: the host answers "no table"
/// for the names that reach the hyphen pass, a final answer. The look stays
/// current, the return costs what it does in English (Senior M1).
#[test]
fn a_missing_hyphenation_table_does_not_make_home_read_its_look_at_every_draw() {
    let _serial = serial();
    let mut device = device_in(false, GERMAN_TOOLS);
    let (all, settings) = back_from_the_drawer(&mut device);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        all <= KEPT_ALL_SLOTS && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads) in German: the look is read again"
    );
    let (steady, steady_reads) = redrawn(&mut device);
    assert!(
        steady <= STEADY_SLOTS && steady_reads <= STEADY_READS,
        "a redraw made {steady} slots ({steady_reads} reads) in German"
    );
}

/// Home's tool names are never hyphenated (#267): a name no line holds ends in
/// "…", and the host is not asked where it may break (on the X4 each ask reads
/// the language's table from the card, 13 ms, 21 in a frame; a frame is drawn
/// in bands and the look is read again after a setting).
#[test]
fn home_does_not_ask_the_host_to_hyphenate_its_tool_names() {
    let _serial = serial();
    let mut device = unstarted(false, GERMAN_TOOLS);
    slot_counts::reset();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    let counts = slot_counts::snapshot();
    let at = |slot: &str| {
        counts
            .iter()
            .find(|(name, _)| *name == slot)
            .map_or(0, |c| c.1)
    };
    assert!(at("text_measure") > 0, "Home did not draw: {counts:?}");
    assert_eq!(
        at("text_hyphenate"),
        0,
        "Home asked for hyphenation: {counts:?}"
    );
}

#[test]
fn a_setting_changed_under_the_cover_is_laid_out_when_it_returns() {
    let _serial = serial();
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
    let _serial = serial();
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
    let ((all, settings), (cover, cover_settings)) = returned(&mut device);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        all <= KEPT_ALL_SLOTS && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
    assert!(
        cover <= COVER_READY_SLOTS && cover_settings <= KEPT_SETTINGS,
        "the ready cover made {cover} host slots ({cover_settings} settings reads): more than its cell"
    );
    assert_ne!(
        shot(&mut device, "card-after"),
        before,
        "the card shows the old progress"
    );
}

#[test]
fn a_plain_setting_changed_just_before_back_shows_when_it_returns() {
    let _serial = serial();
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
    let _serial = serial();
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
    let ((all, settings), (cover, cover_settings)) = returned(&mut device);
    assert_eq!(screen(&mut device), "home.dashboard");
    // A return that lays the page out again reads over 550 settings (614 and
    // 1,298 slots measured); the widgets of the new minute draw a few slots
    // more than the first return does.
    assert!(
        all <= KEPT_ALL_SLOTS + 100 && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
    assert!(
        cover <= COVER_READY_SLOTS && cover_settings <= KEPT_SETTINGS,
        "the ready cover made {cover} host slots ({cover_settings} settings reads): more than its cell"
    );
}

/// The host slots and the settings reads of `redraw` (the whole screen drawn
/// again; the X4 draws it in five bands), as `counted` counts.
fn redrawn(device: &mut Device) -> (u32, u32) {
    counted(device, &["redraw"])
}

/// The return from a book in two parts, as `counted` counts each: the Back up
/// to the first frame of Home, and the wait after it, in which the library
/// announces the covers that became ready and Home draws their cells.
fn returned(device: &mut Device) -> ((u32, u32), (u32, u32)) {
    let first = counted(device, &["back"]);
    let then = counted(device, &["wait:3000"]);
    (first, then)
}

#[test]
fn a_changed_setting_makes_home_read_its_look_again_once() {
    let _serial = serial();
    let mut device = device(false);
    // A page just shown is laid out again by the first change that reaches it
    // (the show's window); a change after that finds the page and the widgets
    // as they are and must have them read their look again.
    device.call(|kernel, now| {
        kernel
            .set_setting(now, "home.clock.frame", "core.frame.line")
            .unwrap()
    });
    run_script(&mut device, "wait:2000,redraw").unwrap();
    // The look was read by the draws; a draw of five bands reads none again.
    run_script(&mut device, "redraw").unwrap();
    let (steady, steady_reads) = redrawn(&mut device);
    eprintln!("steady redraw {steady} slots, {steady_reads} reads");
    assert!(steady <= STEADY_SLOTS && steady_reads <= STEADY_READS);
    let before = shot(&mut device, "look-before");
    // The change, the event that reaches Home and what is drawn after it.
    slot_counts::reset();
    device.call(|kernel, now| {
        kernel
            .set_setting(now, "home.continue.frame", "core.frame.dark")
            .unwrap()
    });
    run_script(&mut device, "wait:1000,redraw").unwrap();
    let counts = slot_counts::snapshot();
    let changed: u32 = counts.iter().map(|(_, n)| *n).sum();
    let changed_reads = counts
        .iter()
        .find(|(name, _)| *name == "settings_read")
        .map_or(0, |(_, n)| *n);
    eprintln!("changed redraw {changed} slots, {changed_reads} reads");
    assert!(
        changed_reads >= CHANGED_READS,
        "the look of Home and of its widgets was not read again: {changed_reads} reads"
    );
    assert_ne!(
        shot(&mut device, "look-after"),
        before,
        "the frame of the card does not show"
    );
    let (again, again_reads) = redrawn(&mut device);
    eprintln!("next redraw {again} slots, {again_reads} reads");
    assert!(again <= STEADY_SLOTS + 40 && again_reads <= STEADY_READS);
}

#[test]
fn repeated_returns_do_not_grow_the_heap() {
    let _serial = serial();
    let mut device = device(false);
    let mut live = 0;
    for round in 0..14 {
        back_from_the_drawer(&mut device);
        if round == 3 {
            live = paperpaw_emulator::budget::stats().live;
        }
    }
    let now = paperpaw_emulator::budget::stats().live;
    eprintln!("HEAP returns: live {live} after 4 returns, {now} after 14");
    assert!(
        now <= live + 256,
        "the heap grew by {} B over ten returns",
        now.saturating_sub(live)
    );
}
