//! The X4's host does not wire `text.ink` (slot 30 answers
//! `ERR_UNSUPPORTED` there until it is timed on the device,
//! `abi/screens-input.md`), which no emulator profile did: on the device
//! every plugin screen that centres text or an icon by its ink failed its
//! whole draw and stayed white (home, the menu, the settings; #250). They
//! place text by its metrics there instead.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

/// The dark pixels above the hints at the foot of the screen.
fn ink(app: &mut App) -> usize {
    app.render();
    let mut dark = 0;
    for y in 0..700 {
        for x in 0..480 {
            dark += usize::from(app.dark(x, y));
        }
    }
    dark
}

/// `what` is drawn: not blank, and no plugin's callback ended with an error
/// (what the device logged for each white screen).
fn drawn(app: &mut App, what: &str) {
    assert!(ink(app) > 200, "{what} is blank");
    let failed = crate::native_activation::take_failed_calls();
    assert!(failed.is_empty(), "{what}: {failed:?}");
}

/// Slot 30 answers as on the X4 while this lives, on this test's thread.
struct AsTheX4;

impl AsTheX4 {
    fn new() -> Self {
        paperpaw_core::native_host::answer_text_ink_as_the_x4(true);
        let _ = crate::native_activation::take_failed_calls();
        Self
    }
}

impl Drop for AsTheX4 {
    fn drop(&mut self) {
        paperpaw_core::native_host::answer_text_ink_as_the_x4(false);
    }
}

#[test]
fn the_plugin_screens_draw_where_the_host_does_not_answer_text_ink() {
    let _x4 = AsTheX4::new();
    let config = "[home]\npage-1 = home.continue\n";
    let mut app = App::new(X4, Shared::memory(config, &[]));
    app.home();
    assert!(app.shows("Start reading"), "{:?}", app.labels());
    drawn(&mut app, "home");
    for (action, screen) in [
        (actions::MENU, "the menu"),
        (actions::SETTINGS, "the settings"),
        (actions::LIBRARY, "the library"),
        (Id::new_static("store.open"), "the store"),
        (Id::new_static("power.open"), "power"),
        (Id::new_static("about.open"), "about"),
    ] {
        app.home();
        app.run(action);
        assert_ne!(app.screen(), "home.dashboard", "{screen}");
        drawn(&mut app, screen);
    }
}

/// The first and last dark row of the screen's header band, where its title
/// is drawn.
fn header_rows(app: &mut App) -> (i32, i32) {
    app.render();
    let rows: Vec<i32> = (0..60)
        .filter(|&y| (0..300).any(|x| app.dark(x, y)))
        .collect();
    (*rows.first().unwrap(), *rows.last().unwrap())
}

/// Placed by its metrics, text stands where the host's ink puts it: the
/// header's title was 10 px too low on the X4, the metrics' descent taken
/// for the ink's bottom (#303).
#[test]
fn text_placed_by_metrics_stands_where_the_ink_puts_it() {
    let config = "[home]\npage-1 = home.continue\n";
    let mut shown = Vec::new();
    for unwired in [false, true] {
        let _x4 = unwired.then(AsTheX4::new);
        let mut app = App::new(X4, Shared::memory(config, &[]));
        app.home();
        app.run(actions::SETTINGS);
        shown.push(header_rows(&mut app));
    }
    let (wired, unwired) = (shown[0], shown[1]);
    assert!(
        wired.0.abs_diff(unwired.0) <= 2 && wired.1.abs_diff(unwired.1) <= 2,
        "title rows with ink {wired:?}, without {unwired:?}"
    );
}
