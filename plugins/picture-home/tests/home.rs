//! The picture home screen with the real kernel and the standard app: it
//! takes the place of the built-in home screen only while it is switched
//! on; the picture is made once for the screen and again when its settings
//! change; the bars show always, never or after a press, and the press
//! that shows them does nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

const WIDE: &[u8] = include_bytes!("data/wide.jpg");
const ON: &str = "[plugins]\npicture-home = on\n";
const DIR: &str = ".paperpaw/plugins/picture-home";
const BAKED: &str = ".paperpaw/plugins/picture-home/background.gray";
const NOTE: &str = ".paperpaw/plugins/picture-home/background.txt";

/// A page with nothing on it but the picture and the bars.
const EMPTY: &str = "page-1 = core.space";

fn storage(extra: &str) -> Shared {
    Shared::memory(
        &format!("{ON}[picture-home]\n{extra}\n"),
        &[("wallpapers/wide.jpg", WIDE)],
    )
}

fn open(device: DeviceInfo, extra: &str) -> App {
    let mut app = App::new(device, storage(extra));
    app.home();
    app
}

/// The baked picture's gray at column `x` of row `y`.
fn baked(app: &App, x: u64, y: u64) -> u8 {
    let mut head = [0u8; 12];
    app.storage.read_at(BAKED, 0, &mut head).unwrap();
    assert_eq!(&head[..4], b"PPGI");
    let width = u64::from(u32::from_le_bytes(head[4..8].try_into().unwrap()));
    let mut pixel = [0u8; 1];
    app.storage
        .read_at(BAKED, 12 + y * width + x, &mut pixel)
        .unwrap();
    pixel[0]
}

#[test]
fn the_built_in_home_screen_stays_until_the_plugin_is_on() {
    let mut app = App::new(X4, Shared::memory("", &[]));
    app.home();
    assert_eq!(app.screen(), "home.dashboard");
    let mut app = open(X4, "");
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(app.shows("Start reading"), "{}", app.described());
}

#[test]
fn the_picture_is_made_once_for_the_screen() {
    let app = open(X4, "picture = wide.jpg\nlighten = 0");
    let names = app.storage.names(DIR);
    assert!(names.contains(&"background.gray".to_string()), "{names:?}");
    assert!(!names.contains(&"background.part".to_string()), "{names:?}");
    assert_eq!(app.storage.size(BAKED).unwrap(), 12 + 480 * 800);
    let note = app.storage.text(NOTE).unwrap();
    assert!(note.starts_with("wallpapers/wide.jpg|"), "{note}");
    // Shown again, restarted: the same file, nothing made anew.
    let before = app.storage.size(BAKED).unwrap();
    let mut app = app.restart();
    app.home();
    assert_eq!(app.storage.size(BAKED).unwrap(), before);
    assert_eq!(app.storage.text(NOTE).unwrap(), note);
}

#[test]
fn fill_cuts_the_sides_and_the_place_says_which() {
    // The picture runs from black on the left to white on the right.
    let left = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.left",
    );
    assert!(baked(&left, 10, 400) < 40);
    assert!(baked(&left, 479, 400) < 150);
    let right = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.right",
    );
    assert!(baked(&right, 470, 400) > 232);
    assert!(baked(&right, 0, 400) > 100);
}

#[test]
fn the_whole_picture_has_white_above_and_below() {
    let app = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nfit = picture-home.fit.fit",
    );
    assert_eq!(baked(&app, 10, 5), 255);
    assert_eq!(baked(&app, 10, 794), 255);
    assert!(baked(&app, 3, 400) < 24);
    assert!(baked(&app, 470, 400) > 232);
}

#[test]
fn a_tile_repeats() {
    let app = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nfit = picture-home.fit.tile\nanchor = picture-home.anchor.top-left",
    );
    // Tiles 240 wide: black at the left of each.
    assert!(baked(&app, 3, 40) < 40);
    assert!(baked(&app, 243, 40) < 40);
    assert!(baked(&app, 236, 40) > 200);
}

#[test]
fn lightening_makes_the_picture_paler() {
    let plain = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.left",
    );
    let pale = open(
        X4,
        "picture = wide.jpg\nlighten = 50\nanchor = picture-home.anchor.left",
    );
    assert!(baked(&pale, 10, 400) >= baked(&plain, 10, 400) + 100);
}

#[test]
fn a_changed_setting_makes_the_picture_anew() {
    let mut app = open(X4, "picture = wide.jpg\nlighten = 0");
    let note = app.storage.text(NOTE).unwrap();
    app.set("picture-home.zoom", "200");
    app.home();
    let again = app.storage.text(NOTE).unwrap();
    assert_ne!(note, again);
    assert!(again.contains("|200|"), "{again}");
}

#[test]
fn taking_the_picture_away_takes_the_made_one_away() {
    let mut app = open(X4, "picture = wide.jpg");
    assert!(app.storage.exists(BAKED));
    app.set("picture-home.picture", "");
    app.home();
    assert!(!app.storage.exists(BAKED));
    assert!(!app.storage.exists(NOTE));
}

#[test]
fn a_picture_that_is_none_leaves_the_page_plain_and_is_not_tried_again() {
    let mut app = open(X4, "picture = missing.jpg");
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(!app.storage.exists(BAKED));
    assert!(app.shows("Start reading"), "{}", app.described());
}

/// The bar's look in the tests: a black plate, which shows where the
/// device has no clock or battery to put on it.
const PLATE: &str = "bar.look = picture-home.look.inverted";

/// Whether anything is drawn in the top strip, where the bar goes.
fn ink_at_top(app: &mut App) -> bool {
    (0..480).step_by(2).any(|x| (4..44).any(|y| app.dark(x, y)))
}

#[test]
fn the_bar_shows_always_never_or_after_a_press() {
    let mut on = open(X4, &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.on"));
    assert!(ink_at_top(&mut on));
    let mut off = open(X4, &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.off"));
    assert!(!ink_at_top(&mut off));
    let mut auto = open(
        X4,
        &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.auto"),
    );
    assert!(!ink_at_top(&mut auto));
    // The first press only shows it: the focus does not move.
    auto.press("right");
    assert!(ink_at_top(&mut auto));
    // A few seconds later it is gone.
    auto.now += 5000;
    let effects = auto.kernel.tick(Instant(auto.now));
    auto.settle(effects);
    assert!(!ink_at_top(&mut auto));
}

#[test]
fn the_first_press_that_shows_the_bars_does_nothing_else() {
    let mut app = open(X4, "bar = picture-home.bar.auto");
    assert_eq!(app.focused(), None);
    app.press("right");
    assert_eq!(app.focused(), None, "{}", app.described());
    app.press("right");
    assert!(app.focused().is_some(), "{}", app.described());
}

#[test]
fn a_tap_on_nothing_shows_the_bar_on_touch() {
    let mut app = open(
        TOUCH,
        &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.auto"),
    );
    assert!(!ink_at_top(&mut app));
    app.tap(Point::new(240, 400));
    assert!(ink_at_top(&mut app));
}

#[test]
fn the_picture_shows_behind_the_page_and_not_while_it_is_arranged() {
    let mut app = open(
        X4,
        &format!(
            "{EMPTY}\nbar = picture-home.bar.off\npicture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.left"
        ),
    );
    // Black at the picture's left edge.
    assert!(app.dark(10, 300));
    app.run(Id::new_static("home.edit-pages"));
    assert!(!app.dark(10, 300));
}

#[test]
fn a_widget_is_added_to_the_page() {
    let mut app = open(X4, "");
    app.open_screen(
        Id::new_static("picture-home.add"),
        Params::new()
            .with("page", "0")
            .with("kind", "core.widgets.time"),
    );
    assert_eq!(app.screen(), "picture-home.add");
    for _ in 0..30 {
        if app.focused().is_some_and(|f| f.starts_with("Date")) {
            break;
        }
        app.run(actions::DOWN);
    }
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "picture-home.dashboard");
    let page = app
        .kernel
        .settings()
        .text("picture-home.page-1")
        .to_string();
    assert!(page.contains("home.date"), "{page}");
}

#[test]
fn the_picture_is_chosen_from_the_folder() {
    let mut app = open(X4, "");
    app.open_screen(Id::new_static("picture-home.pick"), Params::new());
    assert!(app.shows("No picture"), "{}", app.described());
    assert!(app.shows("wide.jpg"), "{}", app.described());
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    assert_eq!(
        app.kernel.settings().text("picture-home.picture"),
        "wide.jpg"
    );
}

/// How many different grays the baked picture has along row `y`.
fn grays_in_row(app: &App, y: u64) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for x in 0..480 {
        seen.insert(baked(app, x, y));
    }
    seen.len()
}

#[test]
fn the_picture_is_made_in_dots_where_it_is_to_be_dithered() {
    let automatic = open(X4, "picture = wide.jpg\nlighten = 0");
    assert!(grays_in_row(&automatic, 400) > 2);
    let grays = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nrender = picture-home.render.grays",
    );
    assert!(grays_in_row(&grays, 400) > 2);
    let dots = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nrender = picture-home.render.dither",
    );
    for y in [10, 200, 400, 600] {
        assert!(grays_in_row(&dots, y) <= 2, "row {y}");
    }
}
