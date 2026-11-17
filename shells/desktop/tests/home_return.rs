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
use std::sync::{PoisonError, RwLock, RwLockReadGuard};

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

const BOOK: &str = "books/alice-in-wonderland.epub";

/// The host slots a return may enter, all together and the settings reads
/// (emulator, X4, Home with its default page). Measured with the page and its
/// look kept: 195 slots and 12 reads from the drawer, 222 and 17 from a book
/// (205 and 12, 232 and 16 before the status line kept what it asked of the
/// host, #294; 338 and 22 before the bands of a frame
/// shared what the first one composed; 517-528 and 23-36 in October); with the page kept and its look
/// read at every draw: 857-868 and 333-346 (967-978 and 423-436 before the
/// names and widgets kept theirs); laid out again on every return, as before:
/// 1,349 and 712.
const KEPT_ALL_SLOTS: u32 = 200;
const KEPT_SETTINGS: u32 = 20;
/// The service calls of a return from the drawer (one of Home's own screens):
/// the wifi plugin's, the library's listing and the status line's values; the
/// stack, the registry and the settings the page asked before are kept (#294).
const KEPT_CALLS: usize = 3;
/// The first frame of a return with a full shelf (six books), host slots
/// (emulator): 289 measured, 299 before the status line kept what it asked of
/// the host, 615 before the bands of a frame shared what the first one
/// composed. The emulator draws the five books' placeholders (a title wrapped
/// by measuring its words) where the X4 draws the covers it has made.
const FULL_SHELF_SLOTS: u32 = 295;
/// The step after the return from a book: the book's card has its cover in
/// the first frame (the library lists the book at once, #267), so the page,
/// black and white first, is drawn again in grays once it is calm (Decision
/// 196): the whole page with its look kept (494 slots, 34 settings reads
/// measured: 284 slots, 15 settings reads). Where the cover came after the first
/// frame, Home drew that cell alone (356 slots). The return itself is
/// `KEPT_ALL_SLOTS`; a laid-out page costs over 1,300.
const GRAY_PASS_SLOTS: u32 = 300;
/// A full redraw (five bands) of a page whose look is kept: slots, settings reads
/// (622 and 270 before the look was kept; 188 and 11 measured).
const STEADY_SLOTS: u32 = 200;
const STEADY_READS: u32 = 14;
/// The first lay-out of the page (the first Back out of the start book): host
/// slots and settings reads (emulator, X4). Measured 587 and 182; 711 and 303
/// before the looks of the page, its widgets and its status line shared the
/// interface's settings, and a laid-out page's return makes 520 to 530. 485 and
/// 185 measured now: the cold page asks each of its things once.
const COLD_SLOTS: u32 = 500;
const COLD_SETTINGS: u32 = 190;
/// The first redraw after a setting changed reads the look of the page and of
/// each of its widgets again: 160 reads measured, 142 where the page forgets
/// nothing, 36 where its widgets forget nothing (18 in a steady redraw); 147
/// since the bands of a frame share what its first band composed.
const CHANGED_READS: u32 = 100;
/// A return that lays the page out again reads at least this many settings
/// (614 measured, with a grid of three columns).
const LAID_SETTINGS: u32 = 120;

/// German, and a page of five columns with tools whose names are long enough
/// to reach the hyphen pass (`Einstellungen`).
const GERMAN_TOOLS: &str = "[core]\nlanguage = de\n[home]\ncolumns = 5\n\
    page-1 = tool:core.library 0/0 1x1, tool:core.settings 1/0 1x1, tool:store.open 2/0 1x1\n";

/// The heap's live bytes are the whole process's: the heap test runs alone,
/// the other tests hold this shared while their devices live.
static HEAP: RwLock<()> = RwLock::new(());

fn shared() -> RwLockReadGuard<'static, ()> {
    HEAP.read().unwrap_or_else(PoisonError::into_inner)
}

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
        .with(
            "paperpaw.ini",
            // With a book open the reader would ask the store for the English
            // pack the firmware seeds, and the dialog would cover the page.
            format!(
                "{}{ini}[help]\nshow = false\n",
                if last {
                    "[core]\nhyphenate = off\n"
                } else {
                    ""
                }
            ),
        );
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
    for (plugin, service, op) in slot_counts::service_calls() {
        eprintln!("  service {plugin} {service}.{op}");
    }
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
    let _shared = shared();
    let mut device = device(false);
    let (all, settings) = back_from_the_drawer(&mut device);
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        all <= KEPT_ALL_SLOTS && settings <= KEPT_SETTINGS,
        "the return made {all} host slots ({settings} settings reads): the page was laid out again"
    );
    let calls = slot_counts::service_calls();
    assert!(
        calls.len() <= KEPT_CALLS,
        "the return made {} service calls: {calls:?}",
        calls.len()
    );
}

/// German with only the English hyphenation table: the host answers "no table"
/// for the names that reach the hyphen pass, a final answer. The look stays
/// current, the return costs what it does in English (Senior M1).
#[test]
fn a_missing_hyphenation_table_does_not_make_home_read_its_look_at_every_draw() {
    let _shared = shared();
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
    let _shared = shared();
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
    let _shared = shared();
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
    let _shared = shared();
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
        cover <= GRAY_PASS_SLOTS && cover_settings <= KEPT_SETTINGS,
        "the gray pass made {cover} host slots ({cover_settings} settings reads): the page was laid out again"
    );
    assert_ne!(
        shot(&mut device, "card-after"),
        before,
        "the card shows the old progress"
    );
}

/// The library is asked for the recent books by the card and by the shelf, once
/// each for a frame, not once for each band of it (#267): on the X4 a query is
/// a score of card reads, and the service calls of the first frame took 1.3 s.
/// Measured: 5 queries before, 3 after (the card's, the shelf's and its measure).
#[test]
fn the_return_from_a_book_does_not_ask_the_library_for_each_band() {
    let _shared = shared();
    let mut device = device(true);
    run_script(&mut device, "back").unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    run_script(
        &mut device,
        "open:reader.screen:path=books/alice-in-wonderland.epub",
    )
    .unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    slot_counts::reset();
    run_script(&mut device, "back").unwrap();
    let calls = slot_counts::service_calls();
    let of = |op: &str| {
        calls
            .iter()
            .filter(|(_, service, name)| *service == "core.library" && *name == op)
            .count()
    };
    let queries = of("query");
    // The reader's close, the wifi plugin's call, the library's listing and the
    // status line's values: Home asks the stack and the registry of nothing (#294).
    assert!(calls.len() <= 4, "the return made {} service calls: {calls:?}", calls.len());
    eprintln!("FIRST FRAME service calls: {}", calls.len());
    for (plugin, service, op) in &calls {
        eprintln!("  service {plugin} {service}.{op}");
    }
    assert!(
        queries == 1 && of("book") == 0 && of("cover") <= 1,
        "the return asked the library {queries} times for its books: {calls:?}"
    );
}

/// An X4 whose card holds `count` copies of one book (`books/b0.epub` ...), the
/// first the one last read, the help hint off.
fn with_books(count: usize) -> Device {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").as_slice();
    let mut files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[core]\nhyphenate = off\n[help]\nshow = false\n",
        )
        .with(
            ".paperpaw/plugins/reader/state.ini",
            "[reader]\nlast = books/b0.epub\n",
        );
    for n in 0..count {
        files = files.with(&format!("books/b{n}.epub"), epub);
    }
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    device
}

/// The service calls the first frame of a return from a book makes when the
/// card shows a card and a full shelf (#267): the X4 made 19 to 20 of them, about
/// 37 ms each (9 here before the stack, the registry's status and the status
/// line's settings were kept, #294: 5 now, the reader's close and the wifi
/// plugin's call among them; 18 here before the card, the shelf and the return check shared one
/// listing and kept what the host said of files and covers; the two cover calls
/// left are for covers the emulator host still reports as being made). Counted by what they
/// ask for: the library once for the listing, and a book's file or cover only
/// where Home has not been told it before.
#[test]
fn the_first_frame_of_a_return_makes_few_service_calls_with_a_full_shelf() {
    let mut device = with_books(6);
    run_script(&mut device, "back").unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    // Every book opened once: all of them are recent.
    for n in (0..6).rev() {
        run_script(
            &mut device,
            &format!("open:reader.screen:path=books/b{n}.epub"),
        )
        .unwrap();
        run_script(&mut device, "wait:3000").unwrap();
        run_script(&mut device, "back").unwrap();
        run_script(&mut device, "wait:3000").unwrap();
    }
    assert_eq!(screen(&mut device), "home.dashboard");
    // The return that matters: out of a book that is on the shelf already.
    run_script(&mut device, "open:reader.screen:path=books/b3.epub").unwrap();
    run_script(&mut device, "wait:3000").unwrap();
    slot_counts::reset();
    run_script(&mut device, "back").unwrap();
    let calls = slot_counts::service_calls();
    let slots = slot_counts::snapshot();
    let all: u32 = slots.iter().map(|(_, n)| *n).sum();
    eprintln!("FIRST FRAME slots {all}: {slots:?}");
    eprintln!("FIRST FRAME service calls: {}", calls.len());
    for (plugin, service, op) in &calls {
        eprintln!("  service {plugin} {service}.{op}");
    }
    let of = |what: &str| {
        calls
            .iter()
            .filter(|(plugin, service, op)| format!("{plugin} {service}.{op}") == what)
            .count()
    };
    assert_eq!(screen(&mut device), "home.dashboard");
    assert!(
        of("home core.library.query") <= 1
            && of("home core.library.book") == 0
            && of("home core.library.cover") <= 1
            && of("home core.ui.stack") == 0
            && of("home core.registry.setting") == 0
            && of("home core.registry.status") == 0
            && calls.len() <= 6,
        "the return made {} service calls: {calls:?}",
        calls.len()
    );
    // The widgets measure, wrap and paint once for the bands of the frame, not
    // once for each band (the emulator draws the shelf's placeholders as the
    // device draws its titles): 615 slots before (#294), the X4's 470 are kept
    // pages with covers.
    assert!(
        all <= FULL_SHELF_SLOTS,
        "the first frame made {all} host slots"
    );
}

/// The first Back out of the book Home was opened under lays the page out for
/// the first time (#294): the X4 made 861 host slots and 31 service calls there
/// against 470 and 19 for the return of a laid-out page. The page, each widget
/// and the status line read the interface's settings once between them (nine
/// times 17 reads before), the library is listed once for the measure and the
/// frame, and the name of Settings comes from the catalog, not the core's
/// actions (their pages are service calls).
#[test]
fn the_first_back_to_home_reads_its_settings_and_registry_once() {
    let mut device = device(true);
    let (all, settings) = counted(&mut device, &["back"]);
    assert_eq!(screen(&mut device), "home.dashboard");
    eprintln!("COLD first back: {all} slots, {settings} settings reads");
    assert!(
        settings <= COLD_SETTINGS && all <= COLD_SLOTS,
        "the first Back made {all} host slots ({settings} settings reads)"
    );
    let calls = slot_counts::service_calls();
    let of = |what: &str| {
        calls
            .iter()
            .filter(|(plugin, service, op)| format!("{plugin} {service}.{op}") == what)
            .count()
    };
    // The registry is asked once for the lay-out (the widgets' list is two
    // pages), the library once.
    assert!(
        of("home core.registry.widgets") <= 2
            && of("home core.registry.menu") <= 4
            && of("home core.registry.actions") == 0
            && of("home core.library.query") <= 1,
        "the page asked again: {calls:?}"
    );
}

#[test]
fn a_plain_setting_changed_just_before_back_shows_when_it_returns() {
    let _shared = shared();
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
    let _shared = shared();
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
        cover <= GRAY_PASS_SLOTS && cover_settings <= KEPT_SETTINGS,
        "the gray pass made {cover} host slots ({cover_settings} settings reads): the page was laid out again"
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
    let _shared = shared();
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
    let _alone = HEAP.write().unwrap_or_else(PoisonError::into_inner);
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
