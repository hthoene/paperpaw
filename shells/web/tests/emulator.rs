//! The browser's emulator without a browser: each device starts the
//! standard app and shows it, takes its buttons and touch, takes a data
//! plugin's files and shows them after a restart, and checks them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::Path;
use std::sync::Mutex;

use paperpaw_platform::PointerPhase;
use paperpaw_web::emulator::{Emulator, PROFILES};

/// The heap budget counts every thread's kernel: one device at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn started(profile: &str) -> Emulator {
    let mut emulator = Emulator::new(profile, "en", false).unwrap();
    emulator.start().unwrap();
    emulator.advance(5_000);
    emulator
}

/// The sample data plugin `name`'s files, as the page would hand them in.
fn sample(name: &str) -> Vec<(String, std::rc::Rc<[u8]>)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/plugins")
        .join(name);
    paperpaw_dev::target::DataPlugin::read(&folder)
        .unwrap()
        .files
        .into_iter()
        .collect()
}

#[test]
fn every_device_shows_the_app() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    for profile in PROFILES {
        let mut emulator = started(profile);
        let (width, height) = emulator.size();
        assert_eq!(
            emulator.frame().len(),
            usize::from(width) * usize::from(height) * 4
        );
        let frame = emulator.shown();
        assert_eq!(frame.len(), usize::from(width) * usize::from(height) * 4);
        // Something drawn: not one colour all over.
        assert!(frame.chunks(4).any(|p| p != &frame[..4]), "{profile}");
        let device = emulator.device_json();
        assert!(
            device.contains(&format!("\"id\":\"{profile}\"")),
            "{device}"
        );
    }
}

#[test]
fn the_devices_are_as_their_profiles_say() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let x4 = Emulator::new("xteink-x4", "en", false).unwrap();
    let json = x4.device_json();
    assert!(json.contains("\"grays\":2"), "{json}");
    assert!(json.contains("\"touch\":false"), "{json}");
    assert!(json.contains("\"power\""), "{json}");
    // Its body, with the keys where they sit.
    assert!(json.contains("\"body\":{\"bezel\":["), "{json}");
    assert!(json.contains("\"place\":\"right\""), "{json}");
    let boox = Emulator::new("boox-go-7", "de", true).unwrap();
    let json = boox.device_json();
    assert!(json.contains("\"touch\":true"), "{json}");
    assert!(json.contains("\"dpi\":300"), "{json}");
    assert!(Emulator::new("no-such-device", "en", false).is_err());
    assert!(Emulator::new("xteink-x4", "../en", false).is_err());
}

#[test]
fn buttons_and_touch_are_taken() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut x4 = started("xteink-x4");
    let before = x4.shown();
    assert!(x4.has_button("right"));
    assert!(!x4.has_button("down"));
    assert!(!x4.button("no-such-button", true, 5_000));
    // The home screen's focus moves.
    assert!(x4.button("right", true, 5_000));
    assert!(x4.button("right", false, 5_050));
    x4.advance(10_000);
    assert!(x4.shown() != before, "the press changed nothing");

    let mut boox = started("boox-go-7");
    let before = boox.shown();
    let (width, height) = boox.size();
    // A tap on the home screen's card (start reading) opens the library.
    let (x, y) = (i32::from(width) / 2, i32::from(height) * 34 / 100);
    boox.pointer(PointerPhase::Down, x, y, 5_000);
    boox.pointer(PointerPhase::Up, x, y, 5_050);
    boox.advance(8_000);
    assert!(boox.shown() != before, "the tap changed nothing");
}

/// The settings open on each device the ways a reader would open them:
/// on the X4 from the menu by its buttons, on the Boox from the menu and
/// from the quick panel by touch (a home screen's tool runs the same
/// action as the menu's entry). Each time after a minute in which the
/// device had nothing to do: a short press stays short.
#[test]
#[ignore = "re-enable at native activation (#228, todo 380): the interface-6 Settings is removed (#1011) and the web emulator has no native one yet"]
fn the_settings_open_from_the_menu_and_the_quick_panel() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::cell::Cell::new(5_000);
    let later = |millis: u64| {
        now.set(now.get() + millis);
        now.get()
    };
    let shows = |emulator: &Emulator, prefix: &str| {
        let screen = emulator.screen();
        assert!(
            screen.as_deref().is_some_and(|s| s.starts_with(prefix)),
            "{screen:?}, not {prefix}"
        );
    };

    // The X4: the menu held open on confirm, Settings its seventh entry
    // (after the quick panel).
    let mut x4 = started("xteink-x4");
    x4.button("confirm", true, later(60_000));
    x4.button("confirm", false, later(1_500));
    x4.advance(later(2_000));
    shows(&x4, "menu.");
    for _ in 0..6 {
        x4.button("right", true, later(60_000));
        x4.button("right", false, later(100));
    }
    x4.button("confirm", true, later(60_000));
    x4.button("confirm", false, later(100));
    x4.advance(later(2_000));
    shows(&x4, "settings.");

    // The Boox: a tap in the middle opens the menu, Settings its last
    // entry; a swipe down from the top the quick panel, with Settings.
    let mut boox = started("boox-go-7");
    let (width, height) = boox.size();
    let at = |x: u32, y: u32| {
        (
            i32::try_from(u32::from(width) * x / 1000).unwrap(),
            i32::try_from(u32::from(height) * y / 1000).unwrap(),
        )
    };
    let tap = |boox: &mut Emulator, (x, y): (i32, i32)| {
        boox.pointer(PointerPhase::Down, x, y, later(60_000));
        boox.pointer(PointerPhase::Up, x, y, later(100));
        boox.advance(later(2_000));
    };
    tap(&mut boox, at(500, 714));
    shows(&boox, "menu.");
    tap(&mut boox, at(142, 960));
    shows(&boox, "settings.");
    boox.button("back", true, later(60_000));
    boox.button("back", false, later(100));
    boox.advance(later(2_000));
    shows(&boox, "home.dashboard");

    let (x, top) = at(500, 2);
    boox.pointer(PointerPhase::Down, x, top, later(60_000));
    for step in 1..=8 {
        let y = top + step * i32::from(height) / 16;
        boox.pointer(PointerPhase::Move, x, y, later(16));
    }
    boox.pointer(PointerPhase::Up, x, top + i32::from(height) / 2, later(16));
    boox.advance(later(2_000));
    shows(&boox, "menu.");
    // Settings is the last tile. The quick panel has one row less than with
    // the interface-6 Power plugin (its Power mode row; the native one has no
    // entry until the central wiring lists it, #228), so the tile is higher.
    tap(&mut boox, at(734, 453));
    shows(&boox, "settings.");
}

#[test]
fn a_data_plugin_shows_after_a_restart() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut emulator = started("xteink-x4");
    let before = emulator.shown();
    for (path, bytes) in sample("large-print") {
        emulator.put_file("large-print", &path, &bytes).unwrap();
    }
    emulator.restart();
    emulator.advance(10_000);
    let large = emulator.shown();
    assert!(large != before, "no larger texts");
    // Taken off again: as it was.
    emulator.remove_plugin("large-print");
    emulator.restart();
    emulator.advance(15_000);
    assert!(emulator.shown() == before, "not as before");
    // Names and paths that would leave the plugin's folder are refused.
    assert!(emulator.put_file("../x", "plugin.ini", &[]).is_err());
    assert!(emulator.put_file("x", "../plugin.ini", &[]).is_err());
}

#[test]
fn the_plugin_is_checked_and_the_device_keeps_going() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut emulator = started("xteink-x4");
    assert!(emulator.check(&[], &[]).is_err(), "nothing to check");
    for (path, bytes) in sample("theme-paw") {
        emulator.put_file("theme-paw", &path, &bytes).unwrap();
    }
    emulator.restart();
    let json = emulator.check(&["package", "lint"], &[]).unwrap();
    assert!(json.contains("\"findings\""), "{json}");
    assert!(json.contains("package.size"), "{json}");
    assert!(emulator.check(&["perf"], &[]).is_err());
    // The device runs on after the check.
    assert!(emulator.button("right", true, 5_000));
    emulator.advance(20_000);
    assert!(!emulator.shown().is_empty());
}

/// A picture on the sleep screen of an X4 (256 KiB of heap): what the
/// kernel writes to the card in memory is no heap of the device's.
#[test]
fn a_sleep_picture_fits_the_x4s_heap() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let picture = include_bytes!("data/stripes-480x800-2bit.png");
    let mut emulator = Emulator::new("xteink-x4", "en", false).unwrap();
    let plugin = [
        (
            "plugin.ini",
            &b"[plugin]\nid = nights\nname = Nights\nversion = 1.0.0\n"[..],
        ),
        (
            "settings.ini",
            b"[sleep]\nshow = sleep.show.picture\nfolder = plugins/nights/sleep\n",
        ),
        ("sleep/stripes.png", picture),
    ];
    for (path, bytes) in plugin {
        emulator.put_file("nights", path, bytes).unwrap();
    }
    emulator.start().unwrap();
    emulator.advance(5_000);
    let before = paperpaw_emulator::budget::stats().failed;
    assert!(emulator.button("power", true, 5_000));
    assert!(emulator.button("power", false, 5_050));
    emulator.advance(10_000);
    assert_eq!(
        paperpaw_emulator::budget::stats().failed,
        before,
        "ran out of memory"
    );
}

/// Another device, night mode or the sleep screen, made apart: what is
/// shown stays as it is.
#[test]
fn snapshots_of_other_devices_leave_what_is_shown() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut x4 = started("xteink-x4");
    let shown = x4.shown();
    let (w, h, boox) = x4.snapshot(Some("boox-go-7"), None, false).unwrap();
    assert_eq!((w, h), (1264, 1680));
    assert_eq!(boox.len(), 1264 * 1680 * 4);
    let (_, _, dark) = x4.snapshot(None, Some(true), false).unwrap();
    let dark_pixels = dark.chunks(4).filter(|p| p[0] < 100).count();
    let light_pixels = shown.chunks(4).filter(|p| p[0] < 100).count();
    assert!(dark_pixels > light_pixels * 2, "night mode is dark");
    let (_, _, asleep) = x4.snapshot(None, None, true).unwrap();
    assert!(asleep != shown, "the sleep screen");
    // This one as it was, and still running.
    assert!(x4.shown() == shown);
    assert!(x4.button("right", true, 5_000));
}

#[test]
fn the_frame_is_in_the_tones_of_e_ink() {
    use paperpaw_emulator::panel::{INK, PAPER};
    use paperpaw_web::emulator::eink;
    // Paper stays, as in screenshots; ink is e-ink's black, not a
    // screen's; between and above, in order.
    assert_eq!(eink(PAPER), PAPER);
    assert_eq!(eink(INK), [0x48; 3]);
    assert_eq!(eink([0; 3]), [0x48; 3]);
    let tones: Vec<_> = (0x1e..=255).map(|v| eink([v; 3])).collect();
    assert!(tones.windows(2).all(|w| (0..3).all(|c| w[0][c] <= w[1][c])));
}
