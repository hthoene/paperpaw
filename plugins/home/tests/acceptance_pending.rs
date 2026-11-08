//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! home screen is pinned by `home.rs`, `layouts.rs`, `arrange.rs` and the other
//! suites on a kernel and by the goldens; these cases are the same behaviours
//! through the production loader with the native plugin. They never pass by
//! themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
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
    // The footer items are picked on the host's status items screen, which a
    // setting cannot name as its editor.
    Case {
        name: "home-footer-items-editor",
        golden: None,
        blocked_by: &["EDITOR-foreign-screen"],
    },
    // The first-time hints on the dashboard.
    Case {
        name: "home-help-hints",
        golden: None,
        blocked_by: &["H1"],
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
#[ignore = "BLOCKED EDITOR-foreign-screen: ExtraDecl names only the plugin's own screens or a slot"]
fn home_footer_items_editor() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: help hints are not declared for the native screen yet"]
fn home_help_hints() {
    CASES[4].refuse().unwrap();
}
