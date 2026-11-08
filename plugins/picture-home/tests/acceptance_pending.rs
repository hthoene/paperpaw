//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! screen is pinned by `home.rs` on a kernel and by the goldens; these cases are
//! the same behaviours through the production loader with the native plugin.
//! They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 6] = [
    // The dashboard: the picture behind the plates, the pages of widgets and
    // tools, the bars, the press that shows a hidden bar and does nothing else.
    Case {
        name: "picture-home-dashboard",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // Editing: dragging, sizing, adding, the tools drawer.
    Case {
        name: "picture-home-edit",
        golden: None,
        blocked_by: &["H2", "H4"],
    },
    // Choosing the picture from the folder and making the screen-sized gray
    // file (decode and scale).
    Case {
        name: "picture-home-picture",
        golden: None,
        blocked_by: &["IMG-slots-36-37", "FS"],
    },
    // The items' editor is a screen of the plugin's own, built on the kit's
    // status items view (opened with the setting's id, drawn, taking input).
    Case {
        name: "picture-home-bar-items-editor",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The bar's items drawn by the host's status frame and the hints row.
    Case {
        name: "picture-home-bars",
        golden: None,
        blocked_by: &["H1"],
    },
    // The grip of a shown item dragged with a finger (down, move, up), as on
    // interface 6: the kit's view takes a hold and a tap instead.
    Case {
        name: "picture-home-bar-items-editor-grip-drag",
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
#[ignore = "BLOCKED H1/H3/H4: native screen commands, compose and input are not qualified"]
fn picture_home_dashboard() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2/H4: dialogs and input are not qualified"]
fn picture_home_edit() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED IMG-slots-36-37: image decode and scale are not on integration"]
fn picture_home_picture() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the native screen's commands and input are not qualified"]
fn picture_home_bar_items_editor() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: the status frame and the hints row are not drawn by the native screen yet"]
fn picture_home_bars() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED KIT-grip-drag: the kit's status items view has no pointer drag of the grip yet"]
fn picture_home_bar_items_editor_grip_drag() {
    CASES[5].refuse().unwrap();
}
