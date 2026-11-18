//! #267, Senior M1: what changes in the very call that closes the screen over
//! Home must show when Home returns. A test-only screen is over Home; an action
//! run on it changes a setting and closes it in one callback; the kernel runs the
//! events of that change in the call after the one that shows Home again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_core::Ask;
use paperpaw_core::{Place, Turn};
use paperpaw_sdk::{Canvas, Handled, SettingValue};

const PAGE: &str = "[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 1x1, \
                    tool:core.settings 1/0 1x1\n";

/// The screen the test fixture opens over Home: the action `test-closer.go`
/// run on it changes a setting and closes it, all in the one callback.
pub struct Closer;

impl paperpaw_core::testing::Fake for Closer {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if action.as_str() != "test-closer.go" {
            return Handled::No;
        }
        cx.command(Ask::SetSetting(
            Id::new_static("home.footer.pencil"),
            SettingValue::Bool(false),
        ));
        cx.command(Ask::Back);
        Handled::Yes
    }
}

/// The dark pixels of the whole screen, as last drawn.
fn ink(app: &mut App) -> usize {
    app.render();
    let mut dark = 0;
    for y in 0..800 {
        for x in 0..480 {
            dark += usize::from(app.dark(x, y));
        }
    }
    dark
}

#[test]
fn a_plain_setting_changed_as_the_screen_over_home_closes_shows() {
    let mut app = App::with_closer(X4, Shared::memory(PAGE, &[]));
    app.home();
    let before = ink(&mut app);
    app.now += 100;
    let effects = app.kernel.open_screen(
        Instant(app.now),
        Id::new_static("test-closer.screen"),
        Params::new(),
    );
    app.settle(effects);
    app.render();
    assert_eq!(app.screen(), "test-closer.screen", "{}", app.described());
    app.now += 100;
    let effects = app
        .kernel
        .run_action(Instant(app.now), Id::new_static("test-closer.go"));
    app.render();
    app.settle(effects);
    assert_eq!(app.screen(), "home.dashboard", "{}", app.described());
    let after = ink(&mut app);
    assert_ne!(after, before, "the footer still has the pencil");
}
