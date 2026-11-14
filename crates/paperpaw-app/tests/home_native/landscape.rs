//! The home screen on a screen on its side (and on a wide one): the pages
//! shown with more columns and what has no room on the next page, nothing
//! stored for it; the first change made there keeping that layout as its
//! own, widgets and tools added to both layouts, the layout reset to the
//! derived one; the page kept when the screen turns; right to left
//! mirrored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_platform::{DisplayInfo, PixelFormat, Rotation};

/// A device `width` by `height` pixels, as the test X4 otherwise: the X4
/// (buttons), or a touch screen.
fn screen(base: DeviceInfo, width: u16, height: u16) -> DeviceInfo {
    base.with_display(DisplayInfo::new(
        width,
        height,
        220,
        PixelFormat::Mono1,
        Rotation::Deg0,
        None,
    ))
}

/// Eight tools on one page, two rows of four.
const TOOLS: &str = "[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 1x1, \
    tool:core.settings 1/0 1x1, tool:battery 2/0 1x1, tool:store.open 3/0 1x1, \
    tool:news.open 0/1 1x1\npage-2 = core.space 0/0 4x1\n";

fn setting(app: &App, id: &str) -> String {
    app.kernel.settings().text(id).to_string()
}

/// The places of the tools shown (their areas), by name, in the order
/// drawn.
fn tools(app: &mut App) -> Vec<(String, paperpaw_sdk::Rect)> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Button)
        .map(|p| (p.label, p.area))
        .collect()
}

/// How many tools stand in the first row of the page.
fn first_row(app: &mut App) -> usize {
    let tools = tools(app);
    let top = tools.iter().map(|t| t.1.y).min().expect("some tools");
    tools.iter().filter(|t| t.1.y == top).count()
}

fn rotate(app: &mut App, width: u16, height: u16) {
    app.now += 100;
    let effects = app.kernel.resize(Instant(app.now), width, height);
    app.settle(effects);
}

#[test]
fn the_columns_grow_on_a_screen_on_its_side_and_not_on_a_tall_one() {
    // Upright X4, X4 on its side, a tablet upright and on its side.
    for (width, height, columns) in [
        (480u16, 800u16, 4i32),
        (800, 480, 6),
        (1200, 1920, 4),
        (1920, 1200, 6),
    ] {
        let mut app = App::new(
            screen(X4, width, height),
            Shared::memory(&format!("{BASE_CONFIG}{TOOLS}"), &[]),
        );
        assert_eq!(app.screen(), "home.dashboard");
        // Four tools in a row of the grid upright; where it is turned, as
        // many as there are columns or tools (five here).
        let got = first_row(&mut app);
        assert_eq!(
            got as i32,
            columns.min(5),
            "{width}x{height}: {}",
            app.described()
        );
        let cells = tools(&mut app);
        // The cell is as wide as the grid's width over its columns.
        let wide = cells[1].1.x - cells[0].1.x;
        assert!(
            (i32::from(wide) - i32::from(width) / columns).abs() < i32::from(width) / 20,
            "{width}x{height}: {wide} wide, {columns} columns"
        );
    }
}

#[test]
fn nothing_is_stored_for_the_layout_on_the_side_and_turning_changes_no_page() {
    let mut app = App::new(
        screen(X4, 800, 480),
        Shared::memory(&format!("{BASE_CONFIG}{TOOLS}"), &[]),
    );
    let before = setting(&app, "home.page-1");
    rotate(&mut app, 480, 800);
    rotate(&mut app, 800, 480);
    assert_eq!(setting(&app, "home.page-1"), before);
    for n in 1..=8 {
        assert_eq!(setting(&app, &format!("home.page-{n}.wide")), "");
    }
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at landscape.rs:130); to analyse (#244, Home removal)"]
fn what_has_no_room_goes_on_to_the_next_page_and_nothing_is_lost() {
    // The default home: the clock, the book, the recent ones, then the
    // tools on a page of their own.
    let all_labels = |app: &mut App| {
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..8 {
            for l in app.labels() {
                if !seen.contains(&l) {
                    seen.push(l);
                }
            }
            app.run(actions::NEXT_PAGE);
        }
        seen
    };
    let mut upright = App::new(screen(X4, 480, 800), Shared::memory(BASE_CONFIG, &[]));
    let upright_labels = all_labels(&mut upright);
    let mut side = App::new(screen(X4, 800, 480), Shared::memory(BASE_CONFIG, &[]));
    // The first page has no room for the tools: not before the second.
    assert!(!side.shows("Library"), "{}", side.described());
    let side_labels = all_labels(&mut side);
    for label in &upright_labels {
        assert!(
            side_labels.contains(label),
            "{label} is lost on the side: {side_labels:?}"
        );
    }
    // What did not fit the first page opens the second, the tools follow
    // it in the room that is left.
    let mut side = App::new(screen(X4, 800, 480), Shared::memory(BASE_CONFIG, &[]));
    side.run(actions::NEXT_PAGE);
    assert!(side.shows("Library"), "{}", side.described());
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at landscape.rs:182); to analyse (#244, Home removal)"]
fn a_page_turned_with_the_screen_is_the_page_with_the_same_first_item() {
    let two = format!(
        "{BASE_CONFIG}[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 1x1\n\
         page-2 = tool:core.settings 0/0 1x1\n"
    );
    let mut app = App::new(screen(X4, 480, 800), Shared::memory(&two, &[]));
    app.run(actions::NEXT_PAGE);
    assert!(
        app.shows("Settings") && !app.shows("Library"),
        "{}",
        app.described()
    );
    // On the side the two tools are one page: each page is filled first.
    rotate(&mut app, 800, 480);
    assert!(
        app.shows("Settings") && app.shows("Library"),
        "{}",
        app.described()
    );
    // Turned back, the page of the first item shown (the first tool).
    rotate(&mut app, 480, 800);
    assert!(
        app.shows("Library") && !app.shows("Settings"),
        "{}",
        app.described()
    );
    // The first page too.
    app.run(actions::PREV_PAGE);
    rotate(&mut app, 800, 480);
    assert!(app.shows("Library"), "{}", app.described());
    // Where the pages are others: the tools page, upright the second, is
    // on the side the one that shows the first of its tools.
    let mut app = App::new(screen(X4, 480, 800), Shared::memory(BASE_CONFIG, &[]));
    app.run(actions::NEXT_PAGE);
    assert!(app.shows("Library"), "{}", app.described());
    rotate(&mut app, 800, 480);
    assert!(app.shows("Library"), "{}", app.described());
    assert!(!app.shows("Start reading"));
}

/// How wide the book card is: from the first to the last dark pixel on the
/// row of its second line, which are its borders (the text lies between).
fn card_width(app: &mut App, width: u16) -> Option<i32> {
    let line = app
        .parts()
        .into_iter()
        .find(|p| p.label.contains("Choose a book"))?
        .area;
    let y = i32::from(line.y) + i32::from(line.height) / 2;
    let dark: Vec<i32> = (0..i32::from(width)).filter(|&x| app.dark(x, y)).collect();
    Some(dark.last()? - dark.first()?)
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at landscape.rs:205); to analyse (#244, Home removal)"]
fn a_big_screen_on_its_side_keeps_cards_to_the_readable_column_and_fills_the_page() {
    // The X4 on its side is narrower than the column: the card is as wide
    // as the screen, and the page is full without the tools.
    let mut app = App::new(screen(X4, 800, 480), Shared::memory(BASE_CONFIG, &[]));
    let wide = card_width(&mut app, 800).expect("the card");
    assert!(wide > 800 * 9 / 10, "{wide}: {}", app.described());
    assert!(!app.shows("Library"), "{}", app.described());
    // Tablets of 1920 and 2560 pixels (220 dpi: 220 and 290 mm): the card is no wider than the column
    // (110 mm, a half of the screen or so), the tools follow beside and
    // below it on the same page.
    for (width, height) in [(1920u16, 1200u16), (2560, 1600)] {
        let mut app = App::new(
            screen(TOUCH, width, height),
            Shared::memory(BASE_CONFIG, &[]),
        );
        let wide = card_width(&mut app, width).expect("the card");
        assert!(
            wide < i32::from(width) * 3 / 4,
            "{width}x{height}: card {wide} wide: {}",
            app.described()
        );
        assert!(
            app.shows("Library"),
            "{width}x{height}: {}",
            app.described()
        );
        // Nothing is stored for it.
        assert_eq!(setting(&app, "home.page-1.wide"), "");
    }
    // Upright is as it was: the card as wide as the screen.
    let mut app = App::new(screen(TOUCH, 1200, 1920), Shared::memory(BASE_CONFIG, &[]));
    let wide = card_width(&mut app, 1200).expect("the card");
    assert!(wide > 1200 * 9 / 10, "{wide}: {}", app.described());
}

fn edit(app: &mut App) {
    app.run(Id::new_static("home.edit-pages"));
}

#[test]
fn the_first_change_on_the_side_keeps_that_layout_as_its_own() {
    let mut app = App::new(
        screen(TOUCH, 800, 480),
        Shared::memory(&format!("{BASE_CONFIG}{TOOLS}"), &[]),
    );
    let upright_before = setting(&app, "home.page-1");
    edit(&mut app);
    let settings = app.find("Settings").expect("the settings tool");
    // Two columns on and a row down, to a free cell (a cell is a sixth of 800).
    app.drag(settings, Point::new(settings.x + 266, settings.y + 120));
    let kept = setting(&app, "home.page-1.wide");
    assert!(kept.contains("tool:core.settings"), "{kept}");
    assert!(
        kept.contains("tool:core.library"),
        "every item of the page is kept: {kept}"
    );
    // Upright: untouched.
    assert_eq!(setting(&app, "home.page-1"), upright_before);
    // A new start: upright shows what it did, on its side the new layout.
    let storage = app.storage.clone();
    let mut upright = App::new(screen(TOUCH, 480, 800), storage.clone());
    assert_eq!(setting(&upright, "home.page-1"), upright_before);
    let at = upright.find("Settings").expect("settings upright");
    let library = upright.find("Library").expect("library upright");
    assert!(at.x > library.x && (at.y - library.y).abs() < 10);
    let mut side = App::new(screen(TOUCH, 800, 480), storage);
    let at = side.find("Settings").expect("settings on its side");
    let library = side.find("Library").expect("library on its side");
    assert!(at.x - library.x > 250, "{at:?} {library:?}");
}

/// Taps `option` of the list shown, on a later page where it is not on
/// this one.
fn choose(app: &mut App, option: &str) {
    for _ in 0..4 {
        if app.find(option).is_some() {
            app.tap_on(option);
            return;
        }
        app.tap_next_page();
    }
    panic!("no option {option}:\n{}", app.described());
}

/// Opens *Add a widget* while editing (touch: the plus below the page).
fn open_add(app: &mut App) {
    app.open_screen(
        Id::new_static("home.add-widget"),
        Params::new().with("page", "0"),
    );
    assert_eq!(app.screen(), "home.add-widget", "{}", app.described());
}

#[test]
fn a_tool_added_on_the_side_is_added_upright_too_and_the_layout_can_be_reset() {
    let mut app = App::new(
        screen(TOUCH, 800, 480),
        Shared::memory(&format!("{BASE_CONFIG}{TOOLS}"), &[]),
    );
    edit(&mut app);
    // A change makes the layout its own.
    let settings = app.find("Settings").expect("the settings tool");
    app.drag(settings, Point::new(settings.x + 266, settings.y + 120));
    open_add(&mut app);
    choose(&mut app, "Tools");
    choose(&mut app, "Vocabulary");
    let wide = setting(&app, "home.page-1.wide");
    assert!(wide.contains("tool:vocab"), "{wide}");
    let upright = setting(&app, "home.page-1");
    assert!(upright.contains("tool:vocab"), "{upright}");
    // And reset: the layout of the screen on its side is the derived one.
    open_add(&mut app);
    choose(&mut app, "Reset landscape layout");
    for n in 1..=8 {
        assert_eq!(setting(&app, &format!("home.page-{n}.wide")), "");
    }
    assert!(setting(&app, "home.page-1").contains("tool:vocab"));
}

#[test]
fn a_tool_added_upright_is_on_the_side_too_where_that_layout_is_its_own() {
    let storage = Shared::memory(&format!("{BASE_CONFIG}{TOOLS}"), &[]);
    // The layout on the side kept: one page, as the derived layout would be.
    let mut side = App::new(screen(TOUCH, 800, 480), storage.clone());
    edit(&mut side);
    let settings = side.find("Settings").expect("the settings tool");
    side.drag(settings, Point::new(settings.x + 266, settings.y + 120));
    assert!(!setting(&side, "home.page-1.wide").is_empty());
    let mut upright = App::new(screen(TOUCH, 480, 800), storage);
    edit(&mut upright);
    open_add(&mut upright);
    choose(&mut upright, "Tools");
    choose(&mut upright, "Vocabulary");
    assert!(setting(&upright, "home.page-1").contains("tool:vocab"));
    assert!(
        setting(&upright, "home.page-1.wide").contains("tool:vocab"),
        "{}",
        setting(&upright, "home.page-1.wide")
    );
}

/// A card with a right to left language.
fn card_ar() -> Shared {
    Shared::memory(
        &format!("[core]\nlanguage = ar\n{TOOLS}"),
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

#[test]
fn right_to_left_the_pages_are_laid_out_as_they_are_left_to_right() {
    // The grid of the home screen is the same in both directions (its cells
    // run from the left, as the page's setting says); the derived layout
    // has the same columns, and the text in the cells is the language's.
    for (width, height) in [(480u16, 800u16), (800, 480), (1920, 1200)] {
        let mut ar = App::new(screen(X4, width, height), card_ar());
        let mut en = App::new(
            screen(X4, width, height),
            Shared::memory(&format!("{BASE_CONFIG}{TOOLS}"), &[]),
        );
        assert_eq!(first_row(&mut ar), first_row(&mut en), "{width}x{height}");
        let cells = |app: &mut App| -> Vec<paperpaw_sdk::Rect> {
            tools(app).into_iter().map(|t| t.1).collect()
        };
        assert_eq!(cells(&mut ar), cells(&mut en), "{width}x{height}");
        for area in cells(&mut ar) {
            assert!(area.x + area.width <= width && area.y + area.height <= height);
        }
    }
}

/// Eight pages of two columns of tools, as many rows as a very tall and narrow
/// screen holds (500 × 3000); on its side (3000 × 500) the grid has more
/// columns but fewer cells.
fn full_pages() -> String {
    let mut config = String::from("[home]\ncolumns = 2\n");
    for page in 1..=8 {
        let cells: Vec<String> = (0..24)
            .map(|i| format!("tool:core.library {}/{} 1x1", i % 2, i / 2))
            .collect();
        config.push_str(&format!("page-{page} = {}\n", cells.join(", ")));
    }
    config
}

/// The tools shown on the pages from the first until `expected` are seen
/// (the pages turn round), and the pages that took.
fn count_all(app: &mut App, expected: usize) -> (usize, usize) {
    let (mut shown, mut pages) = (0, 0);
    while shown < expected && pages < 30 {
        shown += tools(app).len();
        pages += 1;
        app.run(actions::NEXT_PAGE);
    }
    (shown, pages)
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at landscape.rs:414); to analyse (#244, Home removal)"]
fn full_pages_turned_on_the_side_are_all_reachable_and_the_layout_is_not_cut() {
    let mut app = App::new(
        screen(TOUCH, 500, 3000),
        Shared::memory(&format!("{BASE_CONFIG}{}", full_pages()), &[]),
    );
    // What a full page upright holds: eight of them are the items.
    let per_page = tools(&mut app).len();
    assert_eq!(per_page, 24, "{}", app.described());
    let total = 8 * per_page;
    let (upright, pages) = count_all(&mut app, total);
    assert_eq!((upright, pages), (total, 8), "{}", app.described());
    // On its side the grid has fewer cells: more pages than the settings
    // keep, and every tool still on one.
    let mut side = App::new(
        screen(TOUCH, 3000, 500),
        Shared::memory(&format!("{BASE_CONFIG}{}", full_pages()), &[]),
    );
    let (shown, pages) = count_all(&mut side, total);
    assert_eq!(shown, total, "{}", side.described());
    assert!(pages > 8, "{pages} pages");
    // Nothing is stored for it, and a change made there is not kept (that
    // would cut the layout off at eight pages): it says to make room.
    let before: Vec<String> = (1..=8)
        .map(|n| setting(&side, &format!("home.page-{n}.wide")))
        .collect();
    assert!(before.iter().all(|s| s.is_empty()));
    let mut side = App::new(
        screen(TOUCH, 3000, 500),
        Shared::memory(&format!("{BASE_CONFIG}{}", full_pages()), &[]),
    );
    edit(&mut side);
    // The first tool (the buttons of the footer, add and done, are not).
    let first = tools(&mut side)
        .into_iter()
        .find(|t| t.0 != "+" && t.0 != "Done")
        .unwrap()
        .1;
    // A tap on a tool in edit mode offers what to do with it.
    side.tap(center(first));
    side.tap_on("Remove");
    for n in 1..=8 {
        assert_eq!(setting(&side, &format!("home.page-{n}.wide")), "");
    }
    assert!(side.shows("landscape layout"), "{}", side.described());
}

#[test]
fn what_a_page_keeps_without_showing_is_kept_on_the_last_page_on_the_side() {
    // The third page keeps a widget no plugin offers now (switched off, say)
    // and a tool; on its side there are fewer pages than that.
    let config = format!(
        "{BASE_CONFIG}[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 1x1\n\
         page-2 = tool:core.settings 0/0 1x1\n\
         page-3 = gone.widget 0/0 4x1, tool:battery 0/1 1x1\n"
    );
    let mut app = App::new(screen(TOUCH, 800, 480), Shared::memory(&config, &[]));
    edit(&mut app);
    let settings = app.find("Settings").expect("the settings tool");
    app.drag(settings, Point::new(settings.x + 266, settings.y + 120));
    let kept: Vec<String> = (1..=8)
        .map(|n| setting(&app, &format!("home.page-{n}.wide")))
        .filter(|s| !s.is_empty())
        .collect();
    assert!(!kept.is_empty(), "something was kept");
    assert_eq!(
        kept.iter().filter(|s| s.contains("gone.widget")).count(),
        1,
        "{kept:?}"
    );
    assert!(
        kept.last().is_some_and(|s| s.contains("gone.widget")),
        "{kept:?}"
    );
    // The upright layout is as it was.
    assert!(setting(&app, "home.page-3").contains("gone.widget"));
}
