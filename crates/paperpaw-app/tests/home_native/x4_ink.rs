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

#[test]
fn the_plugin_screens_draw_where_the_host_does_not_answer_text_ink() {
    // Slot 30 as the X4 answers it, on this test's thread.
    paperpaw_core::native_host::answer_text_ink_as_the_x4(true);
    let config = "[home]\npage-1 = home.continue\n";
    let mut app = App::new(X4, Shared::memory(config, &[]));
    app.home();
    assert!(app.shows("Start reading"), "{:?}", app.labels());
    assert!(ink(&mut app) > 200, "home is blank");
    for (action, screen) in [
        (actions::MENU, "the menu"),
        (actions::SETTINGS, "the settings"),
        (actions::LIBRARY, "the library"),
    ] {
        app.home();
        app.run(action);
        assert_ne!(app.screen(), "home.dashboard", "{screen}");
        assert!(ink(&mut app) > 200, "{screen} is blank");
    }
    paperpaw_core::native_host::answer_text_ink_as_the_x4(false);
}
