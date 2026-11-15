//! The colours of the widgets on the home screen: the words of the empty
//! start card read on a dark card and on a paper the reader chose (they
//! were black on black), and the status line follows the widget's ink.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

/// The home screen with `extra` set for the start card.
fn start_card(extra: &str) -> App {
    let config = format!("{BASE_CONFIG}[home]\npage-1 = home.continue\n{extra}\n");
    let mut app = App::new(X4, Shared::memory(&config, &[]));
    app.home();
    app
}

/// The pixels dark in the area of the part that says `text`, and how many
/// there are in all.
fn dark_in(app: &mut App, text: &str) -> (usize, usize) {
    let part = app
        .parts()
        .into_iter()
        .find(|p| p.label.contains(text))
        .unwrap_or_else(|| panic!("no part says {text}"));
    let area = part.area;
    let mut dark = 0;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            dark += usize::from(app.dark(i32::from(x), i32::from(y)));
        }
    }
    (dark, usize::from(area.width) * usize::from(area.height))
}

#[test]
fn the_empty_start_card_reads_on_a_light_card() {
    let mut app = start_card("");
    let (dark, all) = dark_in(&mut app, "Start reading");
    // The words only: dark pixels, but few.
    assert!(dark > 20 && dark < all / 3, "{dark} of {all}");
}

#[test]
fn the_empty_start_card_reads_on_a_dark_card_and_on_a_paper_the_reader_chose() {
    for extra in [
        "continue.frame = core.frame.dark",
        "continue.paper = core.plate.inverted",
        "continue.paper = core.plate.dark-gray\ncontinue.ink = core.tone.white",
        "continue.frame = core.frame.line\ncontinue.paper = core.plate.inverted",
    ] {
        let mut app = start_card(extra);
        let (dark, all) = dark_in(&mut app, "Start reading");
        // A dark card with light words in it: most of the place is dark,
        // but not all of it, which black on black would be.
        assert!(dark < all, "{extra}: black on black ({dark} of {all})");
        assert!(dark > all / 3, "{extra}: no dark card ({dark} of {all})");
    }
}

/// The status line above the grid shows the time and the battery on every device,
/// as in Beta 3. Its look is read with the registry's `setting` for
/// `home.status.paper`, which the host answers only in a reply of 4,096 bytes
/// (2,048 made it `NoSpace` and left the strip empty, #257).
#[test]
fn the_status_line_draws_in_the_strip_above_the_grid() {
    for memory in [
        paperpaw_platform::MemoryClass::Small,
        paperpaw_platform::MemoryClass::Large,
    ] {
        let mut app = App::new(X4.with_memory(memory), Shared::memory(BASE_CONFIG, &[]));
        app.home();
        // The device tells the time and the charge, as a shell does.
        let status = paperpaw_platform::SystemStatus::new(
            Some(paperpaw_platform::WallClock::new(
                paperpaw_platform::Instant(0),
                1_791_967_260_000,
                Some(120),
            )),
            Some(paperpaw_platform::Battery::new(80, false, None)),
        );
        let effects = app.kernel.set_status(paperpaw_platform::Instant(0), status);
        app.settle(effects);
        app.render();
        let mut dark = 0;
        for y in 0..60 {
            for x in 0..480 {
                dark += usize::from(app.dark(x, y));
            }
        }
        assert!(dark > 20, "{memory:?}: {dark} dark pixels in the strip");
    }
}
