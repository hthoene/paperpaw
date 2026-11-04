//! What a settings page costs to show (#118): on a panel of black and white
//! the kernel looks at a whole-screen frame once more, drawn aside, for
//! pictures to show in grays (`core.pictures-in-grays`, on by default) -
//! unless the screen says it has none. The settings pages have none, so
//! opening a group or going back must draw them no more often than with
//! the pictures' setting off (where that look is not taken).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

/// How many times the settings plugin's code ran while `keys` were pressed
/// on its main page.
fn runs_during(pictures_in_grays: &str, keys: &[&'static str]) -> u32 {
    let config = format!("[core]\nlanguage = en\npictures-in-grays = {pictures_in_grays}\n");
    let mut app = App::new(X4, Shared::memory(&config, &[]));
    app.home();
    app.run(actions::SETTINGS);
    app.render();
    let calls = |app: &App| {
        app.kernel
            .plugin_usage()
            .iter()
            .find(|p| p.plugin.as_str() == "settings")
            .map_or(0, |p| p.calls)
    };
    let before = calls(&app);
    for key in keys {
        app.press(key);
        app.render();
    }
    calls(&app) - before
}

#[test]
fn opening_a_group_does_not_draw_it_aside_for_pictures() {
    // A group (the first after the search), then back to the list.
    let keys = ["right", "confirm", "back"];
    let on = runs_during("on", &keys);
    let off = runs_during("off", &keys);
    assert!(on > 0, "the keys did nothing");
    assert_eq!(on, off, "the settings pages were drawn aside for pictures");
}
