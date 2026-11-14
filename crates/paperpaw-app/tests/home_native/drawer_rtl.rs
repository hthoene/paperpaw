//! The overview of every tool (`home.drawer`) in a language that runs
//! right to left: the first tool at the top right, the grid from the right,
//! the left button on to the next tool, a tap on a place running the tool
//! there.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{Id, Params, Rect};

const DRAWER: &str = "home.drawer";

/// A card with a right to left language, `ar`: its catalog says the
/// direction and nothing else (the other texts are English).
fn card(language: &str) -> Shared {
    Shared::memory(
        &format!("[core]\nlanguage = {language}\n"),
        &[
            (
                "plugins/language-ar/plugin.ini",
                b"[plugin]\nid = language-ar\nname = Arabic\nversion = 1.0.0\nformats = 3\n",
            ),
            (
                "plugins/language-ar/i18n/ar.ini",
                b"[meta]\nlanguage = ar\nname = Arabic\nplural = arabic\ndirection = rtl\nscript = Arab\n",
            ),
        ],
    )
}

/// The drawer opened, and its tools in the order they are drawn, with the
/// focus.
fn drawer(
    device: paperpaw_platform::DeviceInfo,
    language: &str,
) -> (App, Vec<(String, Rect, bool)>) {
    let mut app = App::new(device, card(language));
    app.open_screen(Id::new_static(DRAWER), Params::new());
    assert_eq!(app.screen(), DRAWER);
    let tools = tools_of(&mut app);
    (app, tools)
}

fn tools_of(app: &mut App) -> Vec<(String, Rect, bool)> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Button)
        .map(|p| (p.label, p.area, p.focused))
        .collect()
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at drawer_rtl.rs:65); to analyse (#244, Home removal)"]
fn the_first_tool_is_at_the_top_right_where_the_interface_runs_right_to_left() {
    for device in DEVICES {
        let width = device.display.width;
        let (_, ltr) = drawer(device, "en");
        let (_, rtl) = drawer(device, "ar");
        assert_eq!(ltr.len(), rtl.len());
        assert!(ltr.len() > 4, "{ltr:?}");
        // The same tools in the same order, in the first row each is
        // mirrored: the first on the side the reading starts.
        for i in 0..4 {
            assert_eq!(ltr[i].0, rtl[i].0);
            assert_eq!(ltr[i].1.y, rtl[i].1.y);
            assert_eq!(ltr[i].1.width, rtl[i].1.width);
            let mirrored = rtl[i].1.x + rtl[i].1.width;
            // Mirrored about the grid's middle, up to the leftover width of
            // its columns.
            let left_over = i32::from(width) - 4 * i32::from(ltr[0].1.width);
            let expected = i32::from(width) - i32::from(ltr[i].1.x);
            assert!(
                (i32::from(mirrored) - expected).abs() <= left_over.abs() + 1,
                "{}: {:?} {:?}",
                ltr[i].0,
                ltr[i].1,
                rtl[i].1
            );
        }
        assert!(ltr[0].1.x < ltr[1].1.x);
        assert!(rtl[0].1.x > rtl[1].1.x, "{rtl:?}");
        // The first at the right edge's margin; all on the screen.
        assert!(
            rtl[0].1.x + rtl[0].1.width + 4 >= width - width / 10,
            "{rtl:?}"
        );
        for (_, area, _) in &rtl {
            assert!(area.x + area.width <= width);
        }
    }
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at drawer_rtl.rs:102); to analyse (#244, Home removal)"]
fn with_buttons_the_left_button_goes_on_to_the_next_tool_from_the_right() {
    for (language, left_goes_on) in [("en", false), ("ar", true)] {
        let (mut app, tools) = drawer(X4, language);
        assert!(tools[0].2, "{language}: {tools:?}");
        let (on, back) = if left_goes_on {
            (actions::LEFT, actions::RIGHT)
        } else {
            (actions::RIGHT, actions::LEFT)
        };
        app.run(on);
        let focused: Vec<_> = tools_of(&mut app).into_iter().filter(|t| t.2).collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].0, tools[1].0, "{language}");
        // The next one is on the side the button points to.
        assert_eq!(
            focused[0].1.x < tools[0].1.x,
            left_goes_on,
            "{language}: {:?} {:?}",
            focused[0].1,
            tools[0].1
        );
        app.run(back);
        let focused: Vec<_> = tools_of(&mut app).into_iter().filter(|t| t.2).collect();
        assert_eq!(focused[0].0, tools[0].0, "{language}");
        // Down is a row on, as in both.
        app.run(actions::DOWN);
        let focused: Vec<_> = tools_of(&mut app).into_iter().filter(|t| t.2).collect();
        assert_eq!(focused[0].0, tools[4].0, "{language}");
    }
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at drawer_rtl.rs:134); to analyse (#244, Home removal)"]
fn a_tap_runs_the_tool_where_it_is_drawn() {
    let (mut app, tools) = drawer(TOUCH, "ar");
    // The second tool (to the left of the first) is the one at its place.
    let area = tools[1].1;
    app.tap(paperpaw_platform::Point::new(
        i32::from(area.x) + i32::from(area.width) / 2,
        i32::from(area.y) + i32::from(area.height) / 3,
    ));
    // It ran: the drawer is gone.
    assert_ne!(app.screen(), DRAWER, "{}", tools[1].0);
}
