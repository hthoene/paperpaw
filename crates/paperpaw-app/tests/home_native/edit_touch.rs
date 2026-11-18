//! Arranging the home screen by touch (#215): while it is arranged, a
//! finger that strays (a swipe on nothing, a tap beside a button) must not
//! turn the page or open the menu or the tools, and *Done* and *Add* are
//! buttons a finger finds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

const HOME: &str = "home.dashboard";
/// Two pages: tools and a wide block on the first, a tool alone on the
/// second.
const PAGES: &str = "[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 1x1, tool:core.settings 1/0 1x1, \
                     tool:store.open 0/1 1x1, home.continue 0/3 4x2\n\
                     page-2 = tool:core.library 0/0 1x1\n";

/// The home screen with two pages, being arranged (a long press on the page).
fn arranging() -> App {
    let mut app = App::new(TOUCH, Shared::memory(PAGES, &[]));
    assert_eq!(app.screen(), HOME);
    assert!(app.shows("Plugin store"), "{}", app.described());
    app.long_press(Point::new(400, 300));
    assert!(app.shows("Done"), "{}", app.described());
    app
}

/// Still page 1, still being arranged, still the home screen.
fn stays(app: &mut App, what: &str) {
    assert_eq!(app.screen(), HOME, "{what}");
    assert!(app.shows("Plugin store"), "{what}: {}", app.described());
    assert!(app.shows("Done"), "{what}: {}", app.described());
}

/// Where nothing is: between the tools and the block below them.
fn empty(app: &mut App) -> i32 {
    let files = app.find("Plugin store").unwrap();
    let library = app.find("Library").unwrap();
    files.y + (files.y - library.y)
}

#[test]
fn a_swipe_on_nothing_while_arranging_keeps_the_page() {
    for (from, to) in [(400, 80), (80, 400)] {
        let mut app = arranging();
        let y = empty(&mut app);
        app.drag(Point::new(from, y), Point::new(to, y));
        stays(&mut app, "sideways");
    }
    let mut app = arranging();
    let y = empty(&mut app);
    for (from, to) in [(y, 120), (y - 100, 790), (y, 20)] {
        app.drag(Point::new(240, from), Point::new(240, to));
        stays(&mut app, "up or down");
    }
}

#[test]
fn a_tap_on_nothing_while_arranging_keeps_the_page() {
    let mut app = arranging();
    let y = empty(&mut app);
    for x in [6, 40, 240, 440, 474] {
        app.tap(Point::new(x, y));
        stays(&mut app, "tap");
    }
    app.long_press(Point::new(240, y));
    stays(&mut app, "long press");
}

#[test]
fn done_is_a_button_a_finger_finds() {
    let mut app = arranging();
    let done = app.find("Done").unwrap();
    // A good way left and below the word: on the button, far from the tick
    // that was the whole of it.
    app.tap(Point::new(done.x - 50, done.y + 20));
    assert!(!app.shows("Done"), "{}", app.described());
    assert_eq!(app.screen(), HOME);
    assert!(app.shows("Plugin store"), "{}", app.described());
}

#[test]
fn add_is_a_button_a_finger_finds() {
    let mut app = arranging();
    let plus = app.find("+").unwrap();
    app.tap(Point::new(plus.x + 50, plus.y + 20));
    assert_ne!(app.screen(), HOME, "{}", app.described());
}

#[test]
fn the_buttons_of_the_footer_do_not_touch_each_other() {
    let mut app = arranging();
    let plus = app.find("+").unwrap();
    let done = app.find("Done").unwrap();
    assert!(done.x - plus.x > 200, "{plus:?} {done:?}");
}
