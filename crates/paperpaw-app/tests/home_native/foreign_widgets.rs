//! A home page that carries another plugin's widgets, on a device whose host
//! refuses to lay a widget of another plugin out from inside a draw (the X4
//! until its nested-callback gate is open, #250): the page still draws, with
//! what it can; the widget it cannot place is left out, as one that is gone is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

/// The dark pixels of the page, above the hints at the foot of the screen.
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
fn a_widget_the_host_will_not_place_is_left_out_and_the_page_draws() {
    let config = "[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 1x1, \
                  widgets.battery 1/0 2x1, widgets.day 1/1 2x1, tool:core.settings 3/0 1x1\n";
    let mut app = App::new(X4, Shared::memory(config, &[]));
    app.home();
    let page = app.kernel.settings().text("home.page-1").to_string();
    assert!(page.contains("widgets.day "), "{page}");
    assert!(ink(&mut app) > 200, "home is blank");
    // The tools beside the widgets are there.
    for name in ["Library", "Settings"] {
        assert!(app.shows(name), "{name} is missing: {}", app.described());
    }
}
