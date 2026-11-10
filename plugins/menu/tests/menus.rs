//! The menus with the real kernel and the standard app, on a buttons-only
//! X4 and a touch-only device (ADR 0010): every entry of the main menu
//! and of the tools opens what it says, back and the menu button close a
//! menu, switches change in place, numbers change by buttons and by
//! touch, long menus are paged with every entry reached, an empty menu
//! says so; the reader's menu (the bar at the top) and its panel at the
//! bottom are reached and used by buttons as by touch.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

const MENU: &str = "menu.list";
/// Where the library entry leads: the library is the native `files` plugin's slot
/// filler, which these kernel tests do not register, so the slot shows its placeholder.
const LIBRARY_SLOT: &str = "core.placeholder";
const PANEL: &str = "menu.panel";
const BOOK: &str = "books/alice.txt";

fn storage(config: &str) -> Shared {
    let text = "Alice was beginning to get very tired of sitting by her sister.\n\n".repeat(200);
    Shared::memory(config, &[(BOOK, text.as_bytes())])
}

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

/// The rows of the menu shown.
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label)
        .collect()
}

/// Chooses the entry that says `label`: with buttons down to it (the
/// first press may only show the focus) and confirm, on touch a tap.
fn choose(app: &mut App, label: &str) {
    if touch(app) {
        app.tap_on(label);
        return;
    }
    for _ in 0..60 {
        if app
            .focused()
            .is_some_and(|f| f == label || f.split(", ").next() == Some(label))
        {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no entry {label}:\n{}", app.described());
}

fn main_menu(device: DeviceInfo) -> App {
    let mut app = App::new(device, storage(""));
    app.run(actions::MENU);
    assert_eq!(app.screen(), MENU);
    app
}

#[test]
fn the_main_menu_lists_its_entries_on_both_devices() {
    for device in DEVICES {
        let mut app = main_menu(device);
        let shown: Vec<String> = rows(&mut app);
        assert_eq!(
            shown,
            [
                "Quick panel",
                "Home",
                "Library",
                "Tools",
                "Edit home screen",
                "Night mode",
                "Settings"
            ]
        );
        // Buttons only: the focus shows at once.
        assert_eq!(app.focused().is_some(), !device.input.touch);
    }
}

#[test]
#[ignore = "re-enable at native activation (#228, todo 380): needs the interface-6 Settings, removed here (held until activation)"]
fn every_entry_of_the_main_menu_opens_what_it_says() {
    let expected = [
        ("Quick panel", "menu.panel"),
        ("Home", "home.dashboard"),
        ("Library", LIBRARY_SLOT),
        ("Tools", MENU),
        ("Edit home screen", "home.dashboard"),
        ("Settings", "settings.list"),
    ];
    for device in DEVICES {
        for (entry, screen) in expected {
            let mut app = main_menu(device);
            app.run(actions::LIBRARY);
            app.run(actions::MENU);
            // From the library: the library's own menu; back, then the
            // main menu from home.
            app.run(actions::BACK);
            app.home();
            app.run(actions::MENU);
            choose(&mut app, entry);
            assert_eq!(app.screen(), screen, "{entry}: {}", app.described());
            if entry == "Tools" {
                assert!(app.shows("Plugin store"), "{}", app.described());
            }
        }
    }
}

#[test]
fn the_switch_in_the_menu_changes_in_place_and_the_menu_stays() {
    for device in DEVICES {
        let mut app = main_menu(device);
        assert!(!app.kernel.settings().bool("core.night-mode"));
        choose(&mut app, "Night mode");
        assert_eq!(app.screen(), MENU);
        assert!(app.kernel.settings().bool("core.night-mode"));
        let night = app
            .parts()
            .into_iter()
            .find(|p| p.label == "Night mode")
            .unwrap();
        assert_eq!(night.on, Some(true));
        choose(&mut app, "Night mode");
        assert!(!app.kernel.settings().bool("core.night-mode"));
        // Kept in the configuration.
        assert!(app.storage.config().contains("night-mode = off"));
    }
}

#[test]
fn back_and_the_menu_button_close_the_menu_and_a_tap_outside_too() {
    for device in DEVICES {
        let mut app = main_menu(device);
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
        app.run(actions::MENU);
        app.run(actions::MENU);
        assert_eq!(app.screen(), "home.dashboard");
        if device.input.touch {
            app.run(actions::MENU);
            // Above the menu, which sits at the bottom.
            app.tap(Point::new(240, 40));
            assert_eq!(app.screen(), "home.dashboard");
        }
    }
}

#[test]
fn every_tool_opens_something_and_back_returns() {
    for device in DEVICES {
        let mut app = App::new(device, storage(""));
        app.run(actions::TOOLS);
        let tools = rows(&mut app);
        assert!(tools.len() >= 5, "{tools:?}");
        for tool in tools {
            app.home();
            app.run(actions::TOOLS);
            assert_eq!(app.screen(), MENU);
            choose(&mut app, &tool);
            // An action ran: the menu closed (a screen opened, or what
            // the tool did shows on the home screen).
            assert_ne!(app.screen(), MENU, "{tool}: {}", app.described());
        }
    }
}

#[test]
fn a_submenu_opens_above_and_back_closes_only_it() {
    for device in DEVICES {
        let mut app = main_menu(device);
        choose(&mut app, "Tools");
        assert_eq!(app.kernel.screens().count(), 3, "home, menu, submenu");
        app.run(actions::BACK);
        assert_eq!(app.screen(), MENU);
        assert!(app.shows("Edit home screen"), "{}", app.described());
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

#[test]
fn an_action_from_a_submenu_reaches_the_screen_below_all_menus() {
    for device in DEVICES {
        let mut app = main_menu(device);
        choose(&mut app, "Tools");
        choose(&mut app, "Plugin store");
        assert_eq!(app.screen(), "store.list");
        // The menus are closed: back leads home.
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

/// A menu of the user's: many entries, a number, a choice.
const OWN: &str = "[menus]\ncore.main = core.ui-size, core.library, core.home, menu.style, \
                   core.night-mode\n";

#[test]
fn a_number_in_a_menu_changes_by_buttons_once_chosen() {
    let mut app = App::new(X4, storage(OWN));
    app.run(actions::MENU);
    let size = "core.ui-size";
    let before = app.kernel.settings().int(size);
    // Not chosen: left and right move the focus, as up and down.
    app.run(actions::CONFIRM);
    app.run(actions::RIGHT);
    app.run(actions::RIGHT);
    assert_eq!(app.kernel.settings().int(size), before + 2);
    app.run(actions::LEFT);
    assert_eq!(app.kernel.settings().int(size), before + 1);
    // Done: down moves on again.
    app.run(actions::CONFIRM);
    app.run(actions::DOWN);
    assert_eq!(app.kernel.settings().int(size), before + 1);
    assert!(app.focused().unwrap().starts_with("Library"));
    assert!(app.storage.config().contains("ui-size ="));
    // Back while changing it: the value before again, as on the settings
    // pages.
    app.run(actions::UP);
    app.run(actions::CONFIRM);
    app.run(actions::RIGHT);
    assert_eq!(app.kernel.settings().int(size), before + 2);
    app.run(actions::BACK);
    assert_eq!(app.kernel.settings().int(size), before + 1);
    assert_eq!(app.screen(), MENU);
}

#[test]
fn a_number_in_a_menu_changes_by_its_minus_and_plus_on_touch() {
    let mut app = App::new(TOUCH, storage(OWN));
    app.run(actions::MENU);
    let size = "core.ui-size";
    let before = app.kernel.settings().int(size);
    app.tap_on("+");
    assert_eq!(app.kernel.settings().int(size), before + 1);
    app.tap_on("−");
    app.tap_on("−");
    assert_eq!(app.kernel.settings().int(size), before - 1);
    assert_eq!(app.screen(), MENU);
}

#[test]
fn a_choice_of_two_in_a_menu_switches_in_place() {
    for device in DEVICES {
        let mut app = App::new(device, storage(OWN));
        app.run(actions::MENU);
        choose(&mut app, "Menus, List");
        assert_eq!(app.screen(), MENU);
        assert_eq!(app.kernel.settings().text("menu.style"), "menu.style.tiles");
        assert!(app.shows("Menus, Tiles"), "{}", app.described());
    }
}

#[test]
#[ignore = "re-enable at native activation (#228, todo 380): needs the interface-6 Settings, removed here (held until activation)"]
fn a_choice_of_more_opens_its_list_and_the_value_chosen_is_kept() {
    for device in DEVICES {
        let mut app = App::new(device, storage("[menus]\ncore.main = core.week-start\n"));
        app.run(actions::MENU);
        let entry = rows(&mut app).remove(0);
        let entry = entry.split(", ").next().unwrap().to_string();
        choose(&mut app, &entry);
        assert_eq!(app.screen(), "settings.choose", "{}", app.described());
        let options = rows(&mut app);
        assert_eq!(options.len(), 3, "{options:?}");
        let last = options
            .last()
            .unwrap()
            .split(", ")
            .next()
            .unwrap()
            .to_string();
        choose(&mut app, &last);
        assert_eq!(app.screen(), MENU);
        assert_eq!(
            app.kernel.settings().text("core.week-start"),
            "core.week.sunday"
        );
        assert!(
            app.storage
                .config()
                .contains("week-start = core.week.sunday")
        );
    }
}

/// Every row of the menu shown, page by page: turned with the page
/// buttons, or on touch screens by the pager's arrow at the bottom right.
fn paged(app: &mut App) -> Vec<String> {
    // Page by page until one comes again (the last stays, or the first
    // follows it): entries of the same name each count.
    let mut seen: Vec<String> = Vec::new();
    let mut pages: Vec<Vec<String>> = Vec::new();
    for _ in 0..20 {
        let page = rows(app);
        if pages.contains(&page) {
            break;
        }
        seen.extend(page.iter().cloned());
        pages.push(page);
        if touch(app) {
            app.tap(Point::new(450, 785));
        } else {
            app.run(actions::NEXT_PAGE);
        }
    }
    seen
}

#[test]
fn a_long_menu_is_paged_and_every_entry_is_reached() {
    // Some thirty actions of the app in the main menu.
    let app = App::new(X4, storage(""));
    let ids: Vec<String> = app
        .kernel
        .actions()
        .iter()
        .map(|a| a.id().as_str().to_string())
        .filter(|id| !id.starts_with("core.") && !id.contains("menu"))
        .collect();
    let config = format!("[menus]\ncore.main = {}\n", ids.join(", "));
    // Buttons: down from the first entry, until it comes round again.
    let mut app = App::new(X4, storage(&config));
    app.run(actions::MENU);
    let first_page = rows(&mut app).len();
    let mut walked: Vec<String> = Vec::new();
    for _ in 0..200 {
        let focused = app.focused().unwrap();
        if walked.first() == Some(&focused) {
            break;
        }
        walked.push(focused);
        app.run(actions::DOWN);
    }
    // Those this device offers: far more than a page.
    assert!(
        walked.len() > 30 && walked.len() > first_page * 3,
        "{walked:?}"
    );
    // The same, page by page.
    let mut app = App::new(X4, storage(&config));
    app.run(actions::MENU);
    assert_eq!(paged(&mut app), walked);
    // And by touch, the pager's arrow.
    let mut app = App::new(TOUCH, storage(&config));
    app.run(actions::MENU);
    let touched = paged(&mut app);
    // Touch screens offer what needs no buttons (and not what does).
    let common: Vec<&String> = walked.iter().filter(|w| touched.contains(w)).collect();
    assert!(common.len() + 6 >= walked.len(), "{walked:?}\n{touched:?}");
    assert!(touched.len() > first_page * 3, "{touched:?}");
}

#[test]
fn an_empty_menu_says_so_and_closes() {
    for device in DEVICES {
        let mut app = App::new(device, storage("[menus]\ncore.main =\n"));
        app.run(actions::MENU);
        assert_eq!(app.screen(), MENU);
        assert!(rows(&mut app).is_empty());
        app.run(actions::CONFIRM);
        app.run(actions::DOWN);
        assert_eq!(app.screen(), MENU);
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

#[test]
fn menus_as_tiles_open_as_a_bar_with_every_entry() {
    for device in DEVICES {
        let mut app = App::new(device, storage("[menu]\nstyle = menu.style.tiles\n"));
        app.run(actions::MENU);
        assert_eq!(app.screen(), MENU);
        // Tiles: no rows, the entries' short names.
        assert!(rows(&mut app).is_empty(), "{}", app.described());
        assert!(app.shows("Library"), "{}", app.described());
        if device.input.touch {
            app.tap_on("Library");
        } else {
            // The focus on the first (the quick panel); right to the
            // library, the third.
            app.run(actions::RIGHT);
            app.run(actions::RIGHT);
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), LIBRARY_SLOT, "{}", app.described());
    }
}

// ---- The reader's menu ------------------------------------------------

/// The entries of the reader's bar, in their order: *Font* first (and
/// focused as it opens), *Bookmark* beside *Night*.
const BAR: [&str; 8] = [
    "Font", "Contents", "Search", "Notes", "Night", "Bookmark", "More", "Home",
];

fn reading(device: DeviceInfo) -> App {
    let mut app = App::new(device, storage(""));
    app.open_slot(slots::READER, Params::new().with(params::PATH, BOOK));
    assert_eq!(app.screen(), "reader.screen");
    app
}

/// Opens the reader's menu: confirm, or a tap in the middle of the page.
fn reader_menu(app: &mut App) {
    if touch(app) {
        app.tap(Point::new(240, 400));
    } else {
        app.run(actions::CONFIRM);
    }
    assert_eq!(app.screen(), MENU, "{}", app.described());
}

/// Chooses the bar's entry `label`: right along the bar to it, or a tap.
fn bar_choose(app: &mut App, label: &str) {
    if touch(app) {
        app.tap_on(label);
        return;
    }
    let at = BAR.iter().position(|e| *e == label).unwrap();
    // The focus is on the first entry.
    app.repeat(actions::RIGHT, at);
    app.run(actions::CONFIRM);
}

#[test]
fn the_reader_menu_bar_shows_the_book_and_its_entries() {
    for device in DEVICES {
        let mut app = reading(device);
        reader_menu(&mut app);
        let labels = app.labels();
        for entry in BAR {
            assert!(labels.iter().any(|l| l == entry), "{entry}: {labels:?}");
        }
        assert!(app.shows("/"), "the page: {labels:?}");
        app.run(actions::BACK);
        assert_eq!(app.screen(), "reader.screen");
    }
}

#[test]
fn every_entry_of_the_reader_menu_opens_what_it_says() {
    let expected = [
        ("Contents", "reader.toc"),
        ("Font", PANEL),
        ("More", MENU),
        ("Home", "home.dashboard"),
    ];
    for device in DEVICES {
        for (entry, screen) in expected {
            let mut app = reading(device);
            reader_menu(&mut app);
            bar_choose(&mut app, entry);
            assert_eq!(app.screen(), screen, "{entry}: {}", app.described());
        }
    }
}

#[test]
fn night_in_the_bar_switches_in_place() {
    for device in DEVICES {
        let mut app = reading(device);
        reader_menu(&mut app);
        bar_choose(&mut app, "Night");
        assert_eq!(app.screen(), MENU);
        assert!(app.kernel.settings().bool("core.night-mode"));
    }
}

#[test]
fn the_bar_wraps_round_with_the_buttons() {
    let mut app = reading(X4);
    reader_menu(&mut app);
    // Left from the first entry: the last, home.
    app.run(actions::LEFT);
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "home.dashboard");
}

/// The bar tells which of its entries has the focus, as a button, as the
/// focus moves along it by buttons.
#[test]
fn the_bar_tells_its_focus() {
    let mut app = reading(X4);
    reader_menu(&mut app);
    for entry in BAR.iter().take(3) {
        let parts = app.parts();
        let focus = paperpaw_sdk::semantics::focused(&parts).unwrap();
        assert_eq!((focus.role, focus.label.as_str()), (Role::Button, *entry));
        app.run(actions::RIGHT);
    }
}

/// The control focused in the panel, and the setting row it belongs to
/// (the nearest text before it that names one).
fn focused_control(app: &mut App) -> Option<(String, String)> {
    let parts = app.parts();
    let at = parts.iter().position(|p| p.focused)?;
    let row = parts[..at]
        .iter()
        .rev()
        .find(|p| p.role == Role::Text && !p.label.chars().next().unwrap().is_ascii_digit())
        .map_or_else(String::new, |p| p.label.clone());
    Some((parts[at].label.clone(), row))
}

#[test]
fn the_panel_at_the_bottom_is_worked_by_buttons() {
    let mut app = reading(X4);
    reader_menu(&mut app);
    bar_choose(&mut app, "Font");
    assert_eq!(app.screen(), PANEL);
    let size = "core.font-size";
    let before = app.kernel.settings().int(size);
    // Along the controls to the font size's +.
    for _ in 0..40 {
        if focused_control(&mut app) == Some(("+".into(), "Font size".into())) {
            break;
        }
        app.run(actions::RIGHT);
    }
    assert_eq!(
        focused_control(&mut app),
        Some(("+".to_string(), "Font size".to_string()))
    );
    app.run(actions::CONFIRM);
    app.run(actions::CONFIRM);
    assert_eq!(app.kernel.settings().int(size), before + 2);
    assert_eq!(app.screen(), PANEL, "the panel stays");
    // A switch: justify.
    for _ in 0..40 {
        if app.focused().as_deref() == Some("Justify text") {
            break;
        }
        app.run(actions::RIGHT);
    }
    let justify = app.kernel.settings().bool("core.justify");
    app.run(actions::CONFIRM);
    assert_eq!(app.kernel.settings().bool("core.justify"), !justify);
    // Back closes the panel, back to the book.
    app.run(actions::BACK);
    assert_eq!(app.screen(), "reader.screen");
}

#[test]
fn the_panel_at_the_bottom_is_worked_by_touch() {
    let mut app = reading(TOUCH);
    reader_menu(&mut app);
    bar_choose(&mut app, "Font");
    assert_eq!(app.screen(), PANEL);
    let size = "core.font-size";
    let before = app.kernel.settings().int(size);
    // The first + is the font size's.
    app.tap_on("+");
    assert_eq!(app.kernel.settings().int(size), before + 1);
    // The switches are on a later page of the panel: a tap on the right
    // half of its pager turns to the next.
    for _ in 0..5 {
        if app.find("Justify text").is_some() {
            break;
        }
        let pager = app
            .parts()
            .into_iter()
            .find(|p| p.role == Role::Text && p.label.contains(" / "))
            .unwrap_or_else(|| {
                panic!(
                    "no pager:
{}",
                    app.described()
                )
            });
        let y = i32::from(pager.area.y) + i32::from(pager.area.height) / 2;
        app.tap(Point::new(
            i32::from(app.kernel.device().display.width) * 7 / 8,
            y,
        ));
    }
    let justify = app.kernel.settings().bool("core.justify");
    app.tap_on("Justify text");
    assert_eq!(app.kernel.settings().bool("core.justify"), !justify);
    // A tap above the panel closes it.
    app.tap(Point::new(240, 60));
    assert_eq!(app.screen(), "reader.screen");
}

#[test]
fn a_tap_at_the_bottom_of_a_page_opens_the_panel_directly() {
    let mut app = reading(TOUCH);
    // The lower third, in the middle.
    app.tap(Point::new(240, 650));
    assert_eq!(app.screen(), PANEL, "{}", app.described());
}

/// A card with a right to left language, `ar`: its catalog says the
/// direction and nothing else (the other texts are English).
fn arabic() -> Shared {
    let text = "Alice was beginning to get very tired of sitting by her sister.\n\n".repeat(200);
    Shared::memory(
        "[core]\nlanguage = ar\n",
        &[
            (BOOK, text.as_bytes()),
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

/// Where the bar's entry `label` is.
fn bar_entry(app: &mut App, label: &str) -> paperpaw_sdk::Rect {
    app.parts()
        .into_iter()
        .find(|p| p.role == Role::Button && p.label == label)
        .map(|p| p.area)
        .unwrap_or_else(|| panic!("{label}: {}", app.described()))
}

/// Where the UI runs right to left the bar's first entry is at the right,
/// and the left button goes on along the bar, the right one back.
#[test]
fn the_reader_menu_bar_runs_from_the_right_in_a_right_to_left_language() {
    let mut app = App::new(X4, arabic());
    app.open_slot(slots::READER, Params::new().with(params::PATH, BOOK));
    reader_menu(&mut app);
    let (font, contents) = (bar_entry(&mut app, "Font"), bar_entry(&mut app, "Contents"));
    assert!(font.x > contents.x, "{font:?} {contents:?}");
    // Same row, so the first is still the focus's start.
    assert_eq!(font.y, contents.y);
    // Left goes on to Contents, right back to Font.
    app.run(actions::LEFT);
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "reader.toc", "{}", app.described());
}

/// The reader's bar on a card with `language`; for `ar` the catalog says
/// the direction and nothing else (the other texts are English).
fn reading_in(language: &str) -> App {
    let storage = Shared::memory(
        &format!("[core]\nlanguage = {language}\n"),
        &[
            (
                BOOK,
                "Alice was beginning to get very tired of sitting by her sister.\n\n"
                    .repeat(200)
                    .as_bytes(),
            ),
            (
                "plugins/language-ar/plugin.ini",
                b"[plugin]\nid = language-ar\nname = Arabic\nversion = 1.0.0\nformats = 3\n",
            ),
            (
                "plugins/language-ar/i18n/ar.ini",
                b"[meta]\nlanguage = ar\nname = Arabic\nplural = arabic\ndirection = rtl\nscript = Arab\n",
            ),
        ],
    );
    let mut app = App::new(X4, storage);
    app.open_slot(slots::READER, Params::new().with(params::PATH, BOOK));
    reader_menu(&mut app);
    app
}

/// The dark pixels of the lines over the bar's entries, left and right of
/// the middle.
fn info_ink(app: &mut App) -> (u32, u32) {
    let top = app
        .parts()
        .into_iter()
        .filter(|p| p.role == Role::Button)
        .map(|p| i32::from(p.area.y))
        .min()
        .unwrap();
    let width = i32::from(app.kernel.device().display.width);
    let (mut left, mut right) = (0, 0);
    for y in 0..top {
        for x in 0..width {
            if app.dark(x, y) {
                if x < width / 2 {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
    }
    (left, right)
}

#[test]
fn the_reader_menu_title_starts_at_the_right_where_the_interface_runs_right_to_left() {
    let (l_left, l_right) = info_ink(&mut reading_in("en"));
    let (r_left, r_right) = info_ink(&mut reading_in("ar"));
    // The title (the bigger text) at the left in English, at the right in
    // Arabic; the page's percentage on the other side.
    assert!(l_left > l_right, "{l_left}/{l_right}");
    assert!(r_right > r_left, "{r_left}/{r_right}");
}

#[test]
fn turned_the_main_menu_stays_open_with_its_focus() {
    let mut app = main_menu(X4);
    app.repeat(actions::DOWN, 2);
    let focused = app.focused().expect("an entry has the focus");
    app.resize(800, 480);
    assert_eq!(app.screen(), MENU);
    assert_eq!(app.focused(), Some(focused.clone()), "landscape");
    for part in app.parts() {
        let area = part.area;
        assert!(
            u32::from(area.x) + u32::from(area.width) <= 800
                && u32::from(area.y) + u32::from(area.height) <= 480,
            "{} is off the screen: {area:?}",
            part.label
        );
    }
    app.resize(480, 800);
    assert_eq!(app.focused(), Some(focused), "upright again");
}

#[test]
fn turned_the_reader_menu_and_its_panel_stay_open() {
    let mut app = reading(X4);
    reader_menu(&mut app);
    let entry = app.focused();
    let bar = app.screen();
    app.resize(800, 480);
    assert_eq!(app.screen(), bar, "the bar stays");
    assert_eq!(app.focused(), entry, "its focus stays");
    bar_choose(&mut app, "Font");
    assert_eq!(app.screen(), PANEL);
    let control = focused_control(&mut app);
    app.resize(480, 800);
    assert_eq!(app.screen(), PANEL, "the panel stays");
    assert_eq!(focused_control(&mut app), control, "its focus stays");
    app.run(actions::BACK);
    assert_eq!(app.screen(), "reader.screen");
}
