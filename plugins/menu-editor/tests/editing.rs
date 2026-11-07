//! Customising menus with the real kernel and the standard app, on a
//! buttons-only X4 and a touch-only device: entries moved up and down
//! (by their options, or dragged by the grip), removed and added, a menu
//! and all menus reset; every change kept in `[menus]` of `paperpaw.ini`
//! and read back by a new kernel, whose menu shows it; and the way to
//! the settings is never removed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

const MENUS: &str = "menu-editor.menus";
const ENTRIES: &str = "menu-editor.entries";
const ENTRY: &str = "menu-editor.entry";
const MAIN: [&str; 7] = [
    "Quick panel",
    "Home",
    "Library",
    "Tools",
    "Edit home screen",
    "Night mode",
    "Settings",
];

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

/// The rows shown, by the first part of their labels.
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

/// Chooses the row that says `label`: down to it and confirm, or a tap
/// (on a later page if needed).
fn choose(app: &mut App, label: &str) {
    if touch(app) {
        for _ in 0..20 {
            if rows(app).iter().any(|r| r == label) {
                app.tap_on(label);
                return;
            }
            app.tap_next_page();
        }
    } else {
        for _ in 0..300 {
            if app
                .focused()
                .is_some_and(|f| f.split(", ").next() == Some(label))
            {
                app.run(actions::CONFIRM);
                return;
            }
            app.run(actions::DOWN);
        }
    }
    panic!("no row {label}:\n{}", app.described());
}

/// Every row of the list, page by page (whole labels).
fn every_row(app: &mut App) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..10 {
        for part in app.parts() {
            if part.role == Role::Row && !seen.contains(&part.label) {
                seen.push(part.label);
            }
        }
        if touch(app) {
            app.tap_next_page();
        } else {
            app.run(actions::NEXT_PAGE);
        }
    }
    seen
}

/// The editor of the main menu.
fn main_menu_editor(device: DeviceInfo, storage: Shared) -> App {
    let mut app = App::new(device, storage);
    app.run(Id::new_static("menu-editor.open"));
    assert_eq!(app.screen(), MENUS);
    choose(&mut app, "Main menu");
    assert_eq!(app.screen(), ENTRIES, "{}", app.described());
    app
}

/// The main menu as it shows from the home screen.
fn shown_main_menu(app: &mut App) -> Vec<String> {
    app.home();
    app.run(actions::MENU);
    let shown = rows(app);
    app.run(actions::BACK);
    shown
}

#[test]
fn the_list_of_menus_has_submenus_after_their_menu() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory("", &[]));
        app.run(Id::new_static("menu-editor.open"));
        let menus = rows(&mut app);
        let main = menus.iter().position(|m| m == "Main menu").unwrap();
        assert_eq!(menus[main + 1], "Main menu › Tools", "{menus:?}");
        // Nothing changed: no reset.
        assert!(!menus.iter().any(|m| m == "Reset all menus"));
    }
}

#[test]
fn an_entry_moved_up_is_kept_and_read_back_by_a_new_kernel() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = main_menu_editor(device, storage.clone());
        let mut expected: Vec<String> = MAIN.iter().map(|s| s.to_string()).collect();
        assert_eq!(rows(&mut app)[..7], expected[..]);
        choose(&mut app, "Library");
        assert_eq!(app.screen(), ENTRY);
        choose(&mut app, "Move up");
        assert_eq!(app.screen(), ENTRIES);
        expected.swap(1, 2);
        assert_eq!(rows(&mut app)[..7], expected[..]);
        // The focus went along (buttons).
        if !device.input.touch {
            assert_eq!(app.focused().as_deref(), Some("Library"));
        }
        let config = storage.config();
        assert!(
            config.contains("[menus]\ncore.main = core.quick-panel, core.library, core.home,"),
            "{config}"
        );
        assert_eq!(shown_main_menu(&mut app), expected);
        let mut app = app.restart();
        assert_eq!(shown_main_menu(&mut app), expected);
    }
}

#[test]
fn moving_past_either_end_changes_nothing() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = main_menu_editor(device, storage.clone());
        choose(&mut app, "Quick panel");
        choose(&mut app, "Move up");
        choose(&mut app, "Settings");
        choose(&mut app, "Move down");
        assert_eq!(app.screen(), ENTRIES);
        assert_eq!(rows(&mut app)[..7], MAIN[..]);
        assert!(
            !storage.config().contains("[menus]"),
            "{}",
            storage.config()
        );
    }
}

#[test]
fn an_entry_removed_is_gone_from_the_menu_and_stays_gone() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = main_menu_editor(device, storage.clone());
        choose(&mut app, "Night mode");
        choose(&mut app, "Remove");
        let expected = [
            "Quick panel",
            "Home",
            "Library",
            "Tools",
            "Edit home screen",
            "Settings",
        ];
        assert_eq!(shown_main_menu(&mut app), expected);
        assert!(
            storage.config().contains(
                "core.main = core.quick-panel, core.home, core.library, core.tools, home.edit-pages, core.settings"
            ),
            "{}",
            storage.config()
        );
        let mut app = app.restart();
        assert_eq!(shown_main_menu(&mut app), expected);
    }
}

#[test]
fn the_way_to_the_settings_cannot_be_removed() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = main_menu_editor(device, storage.clone());
        choose(&mut app, "Settings");
        choose(&mut app, "Remove");
        assert_eq!(app.screen(), ENTRY);
        assert!(app.shows("the way to the settings"), "{}", app.described());
        // Any press closes the refusal.
        if device.input.touch {
            let region = app.find("This entry").unwrap();
            app.tap(region);
        } else {
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), ENTRIES);
        assert!(shown_main_menu(&mut app).contains(&"Settings".to_string()));
        assert!(!storage.config().contains("[menus]"));
    }
}

#[test]
fn once_another_way_is_there_the_settings_entry_may_go() {
    // Settings in the tools too: the main menu still leads to them.
    let config = "[menus]\ncore.tools = core.settings, news.open\n";
    let mut app = main_menu_editor(X4, Shared::memory(config, &[]));
    choose(&mut app, "Settings");
    choose(&mut app, "Remove");
    assert_eq!(app.screen(), ENTRIES);
    assert!(!shown_main_menu(&mut app).contains(&"Settings".to_string()));
}

#[test]
fn an_entry_added_comes_last() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = main_menu_editor(device, storage.clone());
        choose(&mut app, "Add an entry");
        assert_eq!(app.screen(), "menu-editor.add");
        // Those in the menu already are not offered.
        assert!(!rows(&mut app).iter().any(|r| r == "Library"));
        choose(&mut app, "News");
        assert_eq!(app.screen(), ENTRIES);
        let shown = shown_main_menu(&mut app);
        assert_eq!(
            shown.last().map(String::as_str),
            Some("News"),
            "{shown:?}"
        );
        assert!(
            storage.config().contains(", core.settings, news.open"),
            "{}",
            storage.config()
        );
    }
}

#[test]
fn a_menu_reset_is_as_its_plugins_define_it_again() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = main_menu_editor(device, storage.clone());
        choose(&mut app, "Night mode");
        choose(&mut app, "Remove");
        choose(&mut app, "Reset this menu");
        assert_eq!(rows(&mut app)[..7], MAIN[..]);
        assert!(!rows(&mut app).iter().any(|r| r == "Reset this menu"));
        assert!(
            !storage.config().contains("core.main ="),
            "{}",
            storage.config()
        );
        assert_eq!(shown_main_menu(&mut app), MAIN);
    }
}

#[test]
fn all_menus_reset_at_once() {
    for device in DEVICES {
        let config = "[menus]\ncore.main = core.home, core.settings\n\
                      core.tools = core.settings, news.open\n";
        let storage = Shared::memory(config, &[]);
        let mut app = App::new(device, storage.clone());
        app.run(Id::new_static("menu-editor.open"));
        let changed: Vec<String> = every_row(&mut app)
            .into_iter()
            .filter(|r| r.ends_with("Changed"))
            .collect();
        // The tools are no submenu of the main menu so changed.
        assert_eq!(changed, ["Main menu, Changed", "Tools, Changed"]);
        app.run(actions::BACK);
        app.run(Id::new_static("menu-editor.open"));
        choose(&mut app, "Reset all menus");
        let rows = every_row(&mut app);
        assert!(!rows.iter().any(|r| r.contains("Changed")), "{rows:?}");
        assert!(!rows.iter().any(|r| r == "Reset all menus"), "{rows:?}");
        let config = storage.config();
        assert!(!config.contains("core.main ="), "{config}");
        assert!(!config.contains("core.tools ="), "{config}");
        assert_eq!(shown_main_menu(&mut app), MAIN);
    }
}

#[test]
fn an_entry_dragged_by_its_grip_moves_by_touch() {
    let storage = Shared::memory("", &[]);
    let mut app = main_menu_editor(TOUCH, storage.clone());
    let home = app.find("Home").unwrap();
    let night = app.find("Night mode").unwrap();
    // The grip at the right end of the row.
    app.drag(Point::new(465, home.y), Point::new(465, night.y));
    let shown = rows(&mut app);
    assert_eq!(
        shown[..7],
        [
            "Quick panel",
            "Library",
            "Tools",
            "Edit home screen",
            "Night mode",
            "Home",
            "Settings"
        ]
    );
    assert!(
        storage
            .config()
            .contains("core.main = core.quick-panel, core.library, core.tools, home.edit-pages, core.night-mode, core.home, core.settings"),
        "{}",
        storage.config()
    );
}

#[test]
fn a_submenu_is_edited_as_its_menu() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = App::new(device, storage.clone());
        app.run(Id::new_static("menu-editor.open"));
        choose(&mut app, "Main menu › Tools");
        assert_eq!(app.screen(), ENTRIES);
        let first = rows(&mut app).remove(0);
        choose(&mut app, &first);
        choose(&mut app, "Remove");
        assert!(
            storage.config().contains("core.tools = "),
            "{}",
            storage.config()
        );
        app.home();
        app.run(actions::TOOLS);
        assert!(!rows(&mut app).contains(&first), "{first}");
    }
}

#[test]
fn a_hand_written_unknown_entry_shows_by_its_id_and_can_be_removed() {
    for device in DEVICES {
        let config = "[menus]\ncore.main = no.such-thing, core.home, core.settings\n";
        let storage = Shared::memory(config, &[]);
        let mut app = main_menu_editor(device, storage.clone());
        let shown = rows(&mut app);
        assert_eq!(
            shown,
            [
                "no.such-thing",
                "Home",
                "Settings",
                "Add an entry",
                "Reset this menu"
            ]
        );
        // The menu itself leaves it out.
        assert_eq!(shown_main_menu(&mut app), ["Home", "Settings"]);
        app.run(Id::new_static("menu-editor.open"));
        choose(&mut app, "Main menu");
        choose(&mut app, "no.such-thing");
        choose(&mut app, "Remove");
        let config = storage.config();
        assert!(!config.contains("no.such-thing"), "{config}");
        assert!(
            config.contains("core.main = core.home, core.settings"),
            "{config}"
        );
    }
}
