//! The kernel's editor of lists of widgets (the sleep screen's widgets),
//! opened on its slot, with the real kernel and the home screen's widgets, by buttons and by touch: widgets moved up and down (by
//! their options, or dragged by the grip), removed and added, each change
//! kept in the setting at once and read back by a new kernel; settings it
//! is opened for that are no lists or none at all.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

const LIST: &str = "sleep.widgets";
const CONFIG: &str = "[sleep]\nwidgets = home.clock, home.date, home.continue\n";

fn editor(device: DeviceInfo, storage: Shared) -> App {
    let mut app = App::new(device, storage);
    app.open_slot(slots::EDIT_WIDGETS, Params::new().with(params::ID, LIST));
    assert_eq!(app.screen(), "core.widget-list");
    app
}

fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

fn list(app: &App) -> String {
    app.kernel.settings().text(LIST).to_string()
}

/// Chooses the row or option that says `label`: down to it and confirm,
/// or a tap.
fn choose(app: &mut App, label: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(label);
        return;
    }
    for _ in 0..40 {
        if app
            .focused()
            .is_some_and(|f| f.split(", ").next() == Some(label))
        {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no {label}:\n{}", app.described());
}

#[test]
fn widgets_move_up_and_down_and_are_removed() {
    for device in DEVICES {
        let mut app = editor(device, Shared::memory(CONFIG, &[]));
        assert_eq!(
            rows(&mut app),
            ["Clock", "Date", "Continue reading", "Add a widget"]
        );
        choose(&mut app, "Date");
        assert_eq!(app.screen(), "core.choice-dialog");
        choose(&mut app, "Move up");
        assert_eq!(list(&app), "home.date, home.clock, home.continue");
        assert_eq!(
            rows(&mut app),
            ["Date", "Clock", "Continue reading", "Add a widget"]
        );
        // The first cannot go up, the last not down.
        choose(&mut app, "Date");
        assert!(!app.shows("Move up"), "{}", app.described());
        choose(&mut app, "Move down");
        assert_eq!(list(&app), "home.clock, home.date, home.continue");
        choose(&mut app, "Continue reading");
        assert!(!app.shows("Move down"), "{}", app.described());
        choose(&mut app, "Remove");
        assert_eq!(list(&app), "home.clock, home.date");
        assert_eq!(rows(&mut app), ["Clock", "Date", "Add a widget"]);
    }
}

#[test]
fn a_widget_is_added_at_the_end_and_kept_after_a_restart() {
    for device in DEVICES {
        let storage = Shared::memory(CONFIG, &[]);
        let mut app = editor(device, storage.clone());
        choose(&mut app, "Add a widget");
        assert_eq!(app.screen(), "core.add-widget");
        // Those in the list already are not offered again.
        assert!(!app.shows("Continue reading"), "{}", app.described());
        choose(&mut app, "Recently read");
        assert_eq!(app.screen(), "core.widget-list");
        assert_eq!(
            list(&app),
            "home.clock, home.date, home.continue, home.recent"
        );
        assert!(
            storage
                .config()
                .contains("widgets = home.clock, home.date, home.continue, home.recent"),
            "{}",
            storage.config()
        );
        let app = app.restart();
        assert_eq!(
            list(&app),
            "home.clock, home.date, home.continue, home.recent"
        );
    }
}

#[test]
fn a_widget_dragged_by_its_grip_moves_by_touch() {
    let mut app = editor(TOUCH, Shared::memory(CONFIG, &[]));
    let clock = app.find("Clock").unwrap();
    let last = app.find("Continue reading").unwrap();
    // The grip at the right of the row.
    app.drag(Point::new(465, clock.y), Point::new(465, last.y));
    assert_eq!(list(&app), "home.date, home.continue, home.clock");
    assert_eq!(
        rows(&mut app),
        ["Date", "Continue reading", "Clock", "Add a widget"]
    );
}

#[test]
fn an_empty_list_offers_only_adding() {
    for device in DEVICES {
        let mut app = editor(device, Shared::memory("[sleep]\nwidgets =\n", &[]));
        assert_eq!(rows(&mut app), ["Add a widget"]);
        choose(&mut app, "Add a widget");
        choose(&mut app, "Clock");
        assert_eq!(list(&app), "home.clock");
    }
}

#[test]
fn opened_for_no_setting_or_an_unknown_one_it_changes_nothing() {
    for id in ["", "no.such-setting", "..", "home.page-1x"] {
        for device in DEVICES {
            let storage = Shared::memory(CONFIG, &[]);
            let mut app = App::new(device, storage.clone());
            let before = storage.config();
            app.open_slot(slots::EDIT_WIDGETS, Params::new().with(params::ID, id));
            assert_eq!(app.screen(), "core.widget-list", "{id:?}");
            assert_eq!(rows(&mut app), ["Add a widget"], "{id:?}");
            choose(&mut app, "Add a widget");
            choose(&mut app, "Clock");
            assert_eq!(storage.config(), before, "{id:?}");
            app.run(actions::BACK);
        }
    }
}
