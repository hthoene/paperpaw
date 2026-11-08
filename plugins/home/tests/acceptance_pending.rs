//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! home screen is pinned by `home.rs`, `layouts.rs`, `arrange.rs` and the other
//! suites on a kernel and by the goldens; these cases are the same behaviours
//! through the production loader with the native plugin. They never pass by
//! themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
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
#[ignore = "BLOCKED H1: help hints are not declared for the native screen yet"]
fn home_help_hints() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED KIT-grip-drag: the kit's status items view has no pointer drag of the grip yet"]
fn home_footer_items_editor_grip_drag() {
    CASES[5].refuse().unwrap();
}
