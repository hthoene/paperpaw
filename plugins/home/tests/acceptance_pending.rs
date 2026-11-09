//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! home screen is pinned by `home.rs`, `layouts.rs`, `arrange.rs` and the other
//! suites on a kernel and by the goldens; these cases are the same behaviours
//! through the production loader with the native plugin. They never pass by
//! themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 13] = [
    // The dashboard draws its pages of widgets and tools as before (widgets
    // composed with `ui.compose`), pages by swipe or buttons.
    Case {
        name: "home-dashboard",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Editing: dragging, sizing, adding a widget or tool, the tools drawer.
    Case {
        name: "home-edit",
        golden: None,
        blocked_by: &["H2", "H4"],
    },
    // The built-in widgets (status line, date, clock, continue, recent) and
    // their settings and groups.
    Case {
        name: "home-builtin-widgets",
        golden: None,
        blocked_by: &["WIDGETS-declarations"],
    },
    // The items' editor is a screen of the plugin's own, built on the kit's
    // status items view (opened with the setting's id, drawn, taking input).
    Case {
        name: "home-footer-items-editor",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The grid's columns or rows changed: every page fitted to it, what has no
    // room carried to the next page. Widgets' sizes come from `widget_measure`
    // of an instance the dashboard makes, so this runs with the dashboard.
    Case {
        name: "home-grid-reflow",
        golden: None,
        blocked_by: &["HOME-dashboard-slice", "H3"],
    },
    // The tools drawer: every tool as an icon in a grid.
    Case {
        name: "home-drawer",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The first-time hints on the dashboard.
    Case {
        name: "home-help-hints",
        golden: None,
        blocked_by: &["H1"],
    },
    // The grip of a shown item dragged with a finger (down, move, up), as on
    // interface 6: the kit's view takes a hold and a tap instead.
    Case {
        name: "home-footer-items-editor-grip-drag",
        golden: None,
        blocked_by: &["KIT-grip-drag"],
    },
    // The status line above the grid (its items, drawn by their owners and
    // composed), tapped for the item's action.
    Case {
        name: "home-status-line",
        golden: None,
        blocked_by: &["H1", "HOME-status-line-slice"],
    },
    // The icons of tools (above their names, on or faint as their plugins show
    // them) and of the footer's buttons (the pencil, the actions): laid out and
    // drawn through the host.
    Case {
        name: "home-tool-icons",
        golden: None,
        blocked_by: &["H1"],
    },
    // The widgets show new content: the minute tick and `widget_changed` lay
    // the page out anew and draw it (not subscribed to yet), and the screen
    // turned.
    Case {
        name: "home-widget-updates",
        golden: None,
        blocked_by: &["HOME-subscriptions-slice", "H3"],
    },
    // Adding a widget or tool to a page: the gallery that draws widgets as they
    // would show, the ready-made layouts and the icons of the rows. The list
    // steps, the page actions and the writes are done (unit-tested).
    Case {
        name: "home-add",
        golden: None,
        blocked_by: &["HOME-add-gallery", "517", "H1", "H4"],
    },
    // Arranging while editing: moving and sizing an item with a finger (drag by
    // the item, by its corner), and the options' *Settings* entry. Moving,
    // sizing and removing with the buttons are done (unit-tested).
    Case {
        name: "home-edit-arrange",
        golden: None,
        blocked_by: &["H4", "HOME-item-settings"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn home_dashboard() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs and input are not qualified"]
fn home_edit() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WIDGETS-declarations: the five built-in widgets are not declared yet"]
fn home_builtin_widgets() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the native screen's commands and input are not qualified"]
fn home_footer_items_editor() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOME-dashboard-slice: the pages are fitted by the dashboard, which measures widgets"]
fn home_grid_reflow() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the drawer is native (native_drawer.rs) but nothing is drawn or taken through a host yet"]
fn home_drawer() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: help hints are not declared for the native screen yet"]
fn home_help_hints() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED KIT-grip-drag: the kit's status items view has no pointer drag of the grip yet"]
fn home_footer_items_editor_grip_drag() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOME-status-line-slice: the status line above the grid is not drawn yet"]
fn home_status_line() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: native screen commands are not qualified"]
fn home_tool_icons() {
    CASES[9].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOME-subscriptions-slice: the tick and widget changes are not subscribed to yet"]
fn home_widget_updates() {
    CASES[10].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOME-add-gallery: the gallery of widgets as they would show, the ready-made layouts and the icons (#517) are not ported"]
fn home_add() {
    CASES[11].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H4: the finger's drag of an item and its corner needs the pointer input; the options' Settings entry is not ported"]
fn home_edit_arrange() {
    CASES[12].refuse().unwrap();
}
