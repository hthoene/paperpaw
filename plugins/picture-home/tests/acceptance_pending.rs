//! Acceptance of the interface-7 port, pending on the host. The native
//! behaviours are pinned by the unit tests of the `native_*` modules and
//! `tests/hosted_page.rs`; these cases are the same behaviours through the
//! production loader with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 8] = [
    // The dashboard through the production loader: the pages of widgets and
    // tools on their plates, the tools' and the footer's icons (`icon.glyph`),
    // a widget's cell drawn anew at `event.widget_changed` (the model and the
    // screen are ported and tested without the host), the picture behind them
    // (`picture-home-picture`), the bars and the press that shows a hidden bar
    // (`picture-home-bars`).
    Case {
        name: "picture-home-dashboard",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // Editing: the finger's drag of an item or its corner (native, the cells
    // it makes are unit-tested; the buttons' moving, sizing and removing too).
    // Seen only on a host with a screen stack that delivers pointers.
    Case {
        name: "picture-home-edit",
        golden: None,
        blocked_by: &["H1"],
    },
    // The chosen picture behind the page through the production loader: asked
    // of the host (`image_prepare`), the notice while it is made, drawn first
    // and not while editing, given back when the screen closes. The decisions
    // and the drawing's places are ported and tested without the host
    // (`native_backdrop`: up to 256 tiles, only those in the rows a draw may
    // change; a file replaced under the same name asked for anew by its size
    // and time); the turn, the tone, the render and stretching are written
    // against the host's fields and unseen on a device (`PORTING-7.md`).
    Case {
        name: "picture-home-picture",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Choosing the picture from the folder (the screen `pick`: the rows, the
    // empty folder's words, a choice that sets the picture and closes).
    Case {
        name: "picture-home-pick",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The items' editor is a screen of the plugin's own, built on the kit's
    // status items view (opened with the setting's id, drawn, taking input).
    Case {
        name: "picture-home-bar-items-editor",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // The bar drawn in its strip (the plate, the items of every plugin, a tap on
    // one, shown after a press with the row of hints): native and unit-tested
    // without the host; seen only on a host with a screen stack.
    Case {
        name: "picture-home-bars",
        golden: None,
        blocked_by: &["H1"],
    },
    // The screen `add` through the production loader: the tools and widgets to
    // put on a page, *Add a page*, *Remove this page*, and the *Add* button of
    // the editing footer. The rows, the choices' writes and the screen are
    // ported (unit-tested without the host). The rows have no icons yet: icons
    // are admitted (`icon.glyph`, #517 closed for the dashboard), but the kit's
    // list renderer the screen draws with has none (`PORTING-7.md`).
    Case {
        name: "picture-home-add",
        golden: None,
        blocked_by: &["H1", "H4", "KIT-list-icons"],
    },
    // The grip of a shown item dragged with a finger (down, move, up), as on
    // interface 6: the kit's view takes a hold and a tap instead.
    Case {
        name: "picture-home-bar-items-editor-grip-drag",
        golden: None,
        blocked_by: &["H1", "H4"],
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
#[ignore = "BLOCKED H1: the finger's drag of an item and its corner is native (the cells it makes are unit-tested) but is seen only on a host with a screen stack that delivers pointers to the dashboard"]
fn picture_home_edit() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the native screen's commands and input are not qualified"]
fn picture_home_picture() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the native screen's commands and input are not qualified"]
fn picture_home_pick() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the native screen's commands and input are not qualified"]
fn picture_home_bar_items_editor() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1: the bar is native (unit-tested without the host) but is seen only on a host with a screen stack"]
fn picture_home_bars() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the kit view takes the finger's drag of a grip (unit-tested in paperpaw-ui-kit), but nothing is drawn or taken through a host yet"]
fn picture_home_bar_items_editor_grip_drag() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: the native screen's commands and input are not qualified; the rows' icons are not drawn (the kit's list renderer has none)"]
fn picture_home_add() {
    CASES[6].refuse().unwrap();
}
