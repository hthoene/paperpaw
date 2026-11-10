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
    // room carried to the next page. The handler is native (`native_reflow`);
    // the end-to-end case needs a host that keeps settings.
    Case {
        name: "home-grid-reflow",
        golden: None,
        blocked_by: &["HOST-settings-store"],
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
        blocked_by: &["#818", "H1"],
    },
    // The grip of a shown item dragged with a finger (down, move, up), as on
    // interface 6: the kit's view takes a hold and a tap instead.
    Case {
        name: "home-footer-items-editor-grip-drag",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The status line above the grid (its items, drawn by their owners and
    // composed), tapped for the item's action.
    Case {
        name: "home-status-line",
        golden: None,
        blocked_by: &["HOME-status-line-slice", "H1"],
    },
    // The icons of tools (above their names, on or faint as their plugins show
    // them) and of the footer's buttons (the pencil, the actions): laid out and
    // drawn through the host.
    Case {
        name: "home-tool-icons",
        golden: None,
        blocked_by: &["H1"],
    },
    // The widgets show new content: the minute tick (subscribed to, `refresh`)
    // and `widget_changed` (not delivered by the host yet) lay the page out
    // anew and draw it, and the screen turned.
    Case {
        name: "home-widget-updates",
        golden: None,
        blocked_by: &["event.widget_changed", "H3"],
    },
    // Adding a widget or tool to a page: the ready-made layouts and the icons of
    // the rows. The list steps, the page actions, the writes and the gallery's
    // layout, page keys and fallbacks are done (unit-tested; the gallery's
    // drawing needs a host with a screen stack and composition).
    Case {
        name: "home-add",
        golden: None,
        blocked_by: &["HOME-add-layouts", "H1", "H4"],
    },
    // Arranging while editing: moving and sizing an item with a finger (drag by
    // the item, by its corner). The cells a drag makes, where they are free and
    // moving, sizing and removing with the buttons are done (unit-tested); the
    // finger itself needs a host that delivers pointers to the dashboard on a
    // screen stack.
    Case {
        name: "home-edit-arrange",
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
#[ignore = "BLOCKED H1/H4: H1a navigation commands #820 and screen policy #829 (approved, train 7) and the code activation (todo 90) are open; gestures and pointer input to native screens (H4a) has no PR yet (key layer: #805, #815)"]
fn home_dashboard() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs end to end (H2a, Input) has no PR yet; gestures and pointer input to native screens (H4a) has no PR yet (key layer: #805, #815)"]
fn home_edit() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WIDGETS-declarations: the date, the clock, the continue card and the recent shelf are native (declared and drawn by the plugin); the status line is still the interface-6 `blocks.rs`, its native port is plugin work (composition into the screen is #827)"]
fn home_builtin_widgets() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: H1a navigation commands #820 and screen policy #829 (approved, train 7) and the code activation (todo 90) are open; gestures and pointer input to native screens (H4a) has no PR yet (key layer: #805, #815)"]
fn home_footer_items_editor() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOST-settings-store: the handler is written (native_reflow.rs, tested without a host in native_pages), but the scripted host has no settings store to change columns or rows and read the pages back"]
fn home_grid_reflow() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the drawer is native (native_drawer.rs) but nothing is drawn or taken through a host yet; H1a navigation commands #820 and screen policy #829 (approved, train 7) and the code activation (todo 90) are open; gestures and pointer input to native screens (H4a) has no PR yet (key layer: #805, #815)"]
fn home_drawer() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #818: the native screens declare no help hints yet; the host admits `help.hint` with #818 (draft, todo 328), after which home declares the interface-6 hints (home.tips.*); H1: showing them needs the stack (H1a)"]
fn home_help_hints() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the kit view takes the finger's drag of a grip (unit-tested in paperpaw-ui-kit), but nothing is drawn or taken through a host yet: H1 screens and H4 pointer delivery to a native screen on a real host (Input says the host already forwards raw pointers)"]
fn home_footer_items_editor_grip_drag() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOME-status-line-slice: the status line above the grid is not composed yet (plugin work: the status line is composed through slot 35, which #827 wires, draft); H1: showing it needs the stack"]
fn home_status_line() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: the icons are laid out and ready (`icon.glyph`, #725); drawing them through the host needs H1a navigation commands #820 and screen policy #829 (approved, train 7) and the code activation (todo 90) are open"]
fn home_tool_icons() {
    CASES[9].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED event.widget_changed: the minute tick is subscribed to and redraws the dashboard (done, `refresh`), but the host does not deliver `event.widget_changed` (86) yet (only its codec exists in core); H3: composing the widgets is #827 (draft)"]
fn home_widget_updates() {
    CASES[10].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED HOME-add-layouts: the ready-made layouts and the icons of the rows are not ported (plugin work; the icon admission #517 and the glyph call #624 are merged); the gallery is native but its drawing is seen only on a host with a screen stack and composition; H1/H4: H1a #820/#829 and the code activation (todo 90) are open, pointer input has no PR yet"]
fn home_add() {
    CASES[11].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: the finger's drag of an item and its corner is native (the cells it makes are unit-tested) but is seen only on a host with a screen stack that delivers pointers to the dashboard"]
fn home_edit_arrange() {
    CASES[12].refuse().unwrap();
}
