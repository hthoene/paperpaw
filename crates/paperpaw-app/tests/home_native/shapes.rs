//! The shape of the widgets on the home screen: the gap between them and
//! the corners, line and shadow of a widget (#206).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

/// The home screen with the clock and the start card, `extra` set.
fn two_widgets(core: &str, home: &str) -> App {
    let config =
        format!("{BASE_CONFIG}[core]\n{core}\n[home]\npage-1 = home.clock,home.continue\n{home}\n");
    let mut app = App::new(X4, Shared::memory(&config, &[]));
    app.home();
    app
}

/// Where the start card's words begin, down the screen.
fn card_top(app: &mut App) -> u16 {
    app.parts()
        .into_iter()
        .find(|p| p.label.contains("Start reading"))
        .expect("the start card is there")
        .area
        .y
}

/// How many pixels of the whole screen are dark.
fn dark_pixels(app: &mut App) -> usize {
    let mut dark = 0;
    for y in 0..800 {
        for x in 0..480 {
            dark += usize::from(app.dark(x, y));
        }
    }
    dark
}

#[test]
fn the_gap_between_widgets_follows_the_setting() {
    let small = card_top(&mut two_widgets("widgets.gap = core.gap.small", ""));
    let normal = card_top(&mut two_widgets("", ""));
    let large = card_top(&mut two_widgets("widgets.gap = core.gap.large", ""));
    assert!(small < normal, "small {small}, normal {normal}");
    assert!(normal < large, "normal {normal}, large {large}");
}

#[test]
fn a_widgets_shadow_and_line_are_drawn() {
    let plain = dark_pixels(&mut two_widgets("", "continue.frame = core.frame.card"));
    let shadow = dark_pixels(&mut two_widgets(
        "",
        "continue.frame = core.frame.card\ncontinue.drop-shadow = core.shadow.hard",
    ));
    let thick = dark_pixels(&mut two_widgets(
        "",
        "continue.frame = core.frame.line\ncontinue.frame-line = core.lines.thick",
    ));
    let thin = dark_pixels(&mut two_widgets(
        "",
        "continue.frame = core.frame.line\ncontinue.frame-line = core.lines.thin",
    ));
    assert!(shadow > plain, "shadow {shadow}, plain {plain}");
    assert!(thick > thin, "thick {thick}, thin {thin}");
}

#[test]
fn round_corners_leave_less_of_a_dark_card_dark() {
    let dark_with = |value: &str| {
        let mut app = two_widgets(
            "",
            &format!("continue.frame = core.frame.dark\ncontinue.corners = {value}"),
        );
        dark_pixels(&mut app)
    };
    let square = dark_with("core.corners.square");
    let soft = dark_with("core.corners.soft");
    assert!(square > soft, "square {square}, soft {soft}");
}
