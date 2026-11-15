//! A look chosen for all widgets (corners, frame line, shadow) on the real
//! home screen. A widget reads it by asking the registry whether it has a
//! value of its own (`core.registry` `setting`); the kit's reply buffer was
//! smaller than the op's `max_reply`, the host answered `NoSpace`, and the
//! widget was left out of the page (#257).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

#[test]
fn a_look_chosen_for_all_widgets_leaves_the_start_card_drawn() {
    for all in [
        "widgets.corners = core.corners.square",
        "widgets.frame-line = core.lines.thick",
        "widgets.shadow = core.shadow.hard",
    ] {
        let config = format!("[home]\npage-1 = home.continue\n[core]\n{all}\n");
        let mut app = App::new(X4, Shared::memory(&config, &[]));
        let warnings = app.kernel.take_warnings();
        assert!(warnings.is_empty(), "{all}: {warnings:?}");
        app.home();
        let area = app
            .parts()
            .into_iter()
            .find(|p| p.label.contains("Start reading"))
            .expect("the start card")
            .area;
        let mut dark = 0;
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                dark += usize::from(app.dark(i32::from(x), i32::from(y)));
            }
        }
        assert!(dark > 20, "{all}: {dark} dark pixels in the card");
    }
}
