//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! screen is pinned by `home.rs` on a kernel and by the goldens; these cases are
//! the same behaviours through the production loader with the native plugin.
//! They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 5] = [
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
    // The bar's items are picked on the host's status items screen, which a
    // setting cannot name as its editor.
    Case {
        name: "picture-home-bar-items-editor",
        golden: None,
        blocked_by: &["EDITOR-foreign-screen"],
    },
    // The bar's items drawn by the host's status frame and the hints row.
    Case {
        name: "picture-home-bars",
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
#[ignore = "BLOCKED EDITOR-foreign-screen: ExtraDecl names only the plugin's own screens or a slot"]
fn picture_home_bar_items_editor() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: the status frame and the hints row are not drawn by the native screen yet"]
fn picture_home_bars() {
    CASES[4].refuse().unwrap();
}
