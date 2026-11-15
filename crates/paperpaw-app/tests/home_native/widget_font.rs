//! The font the reader chose for widgets (#262) on the real home screen:
//! the start card's words are set in a family from the card once it is
//! chosen for all widgets, the card's own setting rules over that, and
//! the choice is taken again at the next start.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

const GLOBAL: &str = "core.widgets.font";
const OWN: &str = "home.continue.font";

/// The pack of a font plugin on the card, family Narrow: every letter, at
/// every size a pack has, is half its size wide and has no ink of its own,
/// so words set in it stand elsewhere than in the menus' font.
fn narrow_font() -> Vec<u8> {
    let mut builder = PackBuilder::new();
    for size in [20.0_f32, 24.0, 28.0, 32.0, 36.0, 40.0, 48.0] {
        let metrics = LineMetrics::new(size * 0.8, size * 0.2, size * 1.2);
        let glyphs = (' '..='~')
            .map(|ch| (ch, Glyph::empty(size / 2.0)))
            .collect();
        builder.add_strike(size, metrics, glyphs);
    }
    builder.build()
}

/// The start card as drawn when the reader comes back to the home screen
/// (from the settings, where a font is chosen): which of its pixels are dark.
fn card(app: &mut App) -> Vec<bool> {
    app.run(actions::LIBRARY);
    assert_ne!(app.screen(), "home.dashboard");
    app.home();
    let part = app
        .parts()
        .into_iter()
        .find(|p| p.label.contains("Start reading"))
        .expect("the start card");
    let area = part.area;
    let mut dark = Vec::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            dark.push(app.dark(i32::from(x), i32::from(y)));
        }
    }
    dark
}

#[test]
fn the_start_card_is_set_in_the_family_chosen_for_widgets() {
    let pack = narrow_font();
    let files: [(&str, &[u8]); 2] = [
        (
            "plugins/font-narrow/plugin.ini",
            b"[plugin]\nname = Narrow\n\n[family]\nname = Narrow\nregular = narrow.ppfp\n",
        ),
        ("plugins/font-narrow/fonts/narrow.ppfp", &pack),
    ];
    let config = "[home]\npage-1 = home.continue\n";
    let mut app = App::new(X4, Shared::memory(config, &files));
    let menus = card(&mut app);
    assert!(menus.iter().filter(|dark| **dark).count() > 20);

    // For all widgets: the card's words are set in the family.
    app.set(GLOBAL, "Narrow");
    let narrow = card(&mut app);
    assert!(narrow != menus, "the card did not change");
    // The card's own setting rules over the one for all.
    app.set(OWN, "core.widgets.font.menus");
    assert!(card(&mut app) == menus);
    app.set(OWN, "core.widgets.font.all");
    assert!(card(&mut app) == narrow);

    // Kept: taken again at the next start, without a warning, the family
    // open from then on.
    let mut app = app.restart();
    let warnings = app.kernel.take_warnings();
    assert!(
        warnings.iter().all(|w| !w.message.contains("font")),
        "{warnings:?}"
    );
    assert_eq!(app.kernel.settings().text(GLOBAL), "Narrow");
    assert!(card(&mut app) == narrow);
    app.set(GLOBAL, "core.widgets.font.menus");
    assert!(card(&mut app) == menus);
}
